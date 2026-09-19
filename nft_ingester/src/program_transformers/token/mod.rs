use crate::{config::IngesterConfig, error::IngesterError, metric, tasks::TaskData};
use blockbuster::programs::token_account::TokenProgramAccount;
use cadence_macros::{is_global_default_set, statsd_count};
use digital_asset_types::dao::{asset, owners, sea_orm_active_enums::OwnerType, tokens};
use plerkle_serialization::AccountInfo;
use sea_orm::{
    entity::*, query::*, sea_query::OnConflict, ActiveValue::Set, ConnectionTrait,
    DatabaseConnection, DbBackend, EntityTrait,
};
use solana_sdk::program_option::COption;
use spl_token::state::AccountState;
use tokio::sync::mpsc::UnboundedSender;

use super::asset_upserts::{
    upsert_assets_mint_account_columns, upsert_assets_token_account_columns,
    AssetMintAccountColumns, AssetTokenAccountColumns,
};

pub async fn upsert_owner_for_account<T>(
    txn_or_conn: &T,
    id: Vec<u8>,
    token_account: Option<Vec<u8>>,
    owner: Vec<u8>,
    delegate: Option<Vec<u8>>,
    slot: i64,
    frozen: bool,
    extensions: Option<serde_json::Value>,
    amount: u64,
    delegate_amount: i64,
    token_program: Vec<u8>,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let owners_model = owners::ActiveModel {
        mint: Set(Some(id)),
        token_account: Set(token_account),
        owner: Set(Some(owner)),
        delegate: Set(delegate),
        slot_updated: Set(Some(slot)),
        frozen: Set(frozen),
        token_extensions: Set(extensions),
        token_amount: Set(Some(amount as i64)),
        token_amount_u64: Set(Some(amount.into())),
        delegated_amount: Set(Some(delegate_amount)),
        token_program: Set(Some(token_program)),
        closed: Set(Some(false)),
        ..Default::default()
    };
    let mut query = owners::Entity::insert(owners_model)
        .on_conflict(
            OnConflict::columns([owners::Column::TokenAccount])
                .update_columns([
                    owners::Column::Owner,
                    owners::Column::Mint,
                    owners::Column::Delegate,
                    owners::Column::SlotUpdated,
                    owners::Column::Frozen,
                    owners::Column::TokenExtensions,
                    owners::Column::TokenAmount,
                    owners::Column::TokenAmountU64,
                    owners::Column::DelegatedAmount,
                    owners::Column::TokenProgram,
                    owners::Column::Closed,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    query.sql = format!(
        "{} WHERE (excluded.slot_updated >= owners.slot_updated OR owners.slot_updated IS NULL)
        AND (excluded.owner, excluded.mint, excluded.delegate, excluded.slot_updated,
             excluded.frozen, excluded.token_extensions, excluded.token_amount,
             excluded.token_amount_u64, excluded.delegated_amount, excluded.token_program, excluded.closed)
            IS DISTINCT FROM
            (owners.owner, owners.mint, owners.delegate, owners.slot_updated,
             owners.frozen, owners.token_extensions, owners.token_amount,
             owners.token_amount_u64, owners.delegated_amount, owners.token_program, owners.closed)",
        query.sql
    );
    txn_or_conn
        .execute(query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    Ok(())
}

// Both token programs use the same ordering/no-op rule, but update different
// extension columns. Preserve that distinction instead of clearing unrelated data.
pub(super) fn token_mint_upsert(
    model: tokens::ActiveModel,
    extension_column: tokens::Column,
) -> sea_orm::Statement {
    use sea_orm::sea_query::Iden;
    let extension_name = extension_column.to_string();
    let mut query = tokens::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([tokens::Column::Mint])
                .update_columns([
                    tokens::Column::Supply,
                    tokens::Column::TokenProgram,
                    tokens::Column::MintAuthority,
                    tokens::Column::CloseAuthority,
                    extension_column,
                    tokens::Column::SlotUpdated,
                    tokens::Column::Decimals,
                    tokens::Column::FreezeAuthority,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    query.sql = format!(
        "{} WHERE excluded.slot_updated >= tokens.slot_updated
        AND (excluded.supply, excluded.token_program, excluded.mint_authority,
             excluded.close_authority, excluded.{extension_name}, excluded.slot_updated,
             excluded.decimals, excluded.freeze_authority)
            IS DISTINCT FROM
            (tokens.supply, tokens.token_program, tokens.mint_authority,
             tokens.close_authority, tokens.{extension_name}, tokens.slot_updated,
             tokens.decimals, tokens.freeze_authority)",
        query.sql
    );
    query
}

pub async fn handle_token_program_account<'a, 'b, 'c>(
    account_update: &'a AccountInfo<'a>,
    parsing_result: &'b TokenProgramAccount,
    db: &'c DatabaseConnection,
    _task_manager: &UnboundedSender<TaskData>,
    _config: &IngesterConfig,
) -> Result<(), IngesterError> {
    let key = *account_update.pubkey().unwrap();
    let key_bytes = key.0.to_vec();
    let spl_token_program = account_update.owner().unwrap().0.to_vec();
    match &parsing_result {
        TokenProgramAccount::TokenAccount(ta) => {
            let mint = ta.mint.to_bytes().to_vec();
            let delegate: Option<Vec<u8>> = match ta.delegate {
                COption::Some(d) => Some(d.to_bytes().to_vec()),
                COption::None => None,
            };
            let frozen = match ta.state {
                AccountState::Frozen => true,
                _ => false,
            };
            let owner = ta.owner.to_bytes().to_vec();

            upsert_owner_for_account(
                db,
                mint.clone(),
                Some(key_bytes),
                owner.clone(),
                delegate.clone(),
                account_update.slot() as i64,
                frozen,
                None,
                ta.amount,
                ta.delegated_amount as i64,
                spl_token_program,
            )
            .await?;

            // Metrics
            let mut token_owner_update = false;
            let mut token_delegate_update = false;
            let mut token_freeze_update = false;

            let txn = db.begin().await?;
            let asset_update = asset::Entity::find_by_id(mint.clone())
                .filter(asset::Column::OwnerType.eq("single").and(
                    asset::Column::SlotUpdatedTokenAccount.is_null().or(
                        asset::Column::SlotUpdatedTokenAccount.lte(account_update.slot() as i64),
                    ),
                ))
                .one(&txn)
                .await?;

            if let Some(asset) = asset_update {
                // Only handle token account updates for NFTs (supply=1)
                // TODO: Support fungible tokens
                let asset_clone = asset.clone();
                if asset_clone.supply == 1 {
                    let mut save_required = false;
                    let mut active: asset::ActiveModel = asset.into();

                    // Handle ownership updates
                    let old_owner = asset_clone.owner.clone();
                    let new_owner = owner.clone();
                    if ta.amount > 0 && Some(new_owner) != old_owner {
                        active.owner = Set(Some(owner.clone()));
                        token_owner_update = true;
                        save_required = true;
                    }

                    // Handle delegate updates
                    if ta.amount > 0 && delegate.clone() != asset_clone.delegate {
                        active.delegate = Set(delegate.clone());
                        token_delegate_update = true;
                        save_required = true;
                    }

                    // Handle freeze updates
                    if ta.amount > 0 && frozen != asset_clone.frozen {
                        active.frozen = Set(frozen);
                        token_freeze_update = true;
                        save_required = true;
                    }

                    if save_required {
                        upsert_assets_token_account_columns(
                            AssetTokenAccountColumns {
                                mint,
                                owner: Some(owner),
                                frozen,
                                delegate,
                                token_extensions: None,
                                slot_updated_token_account: Some(account_update.slot() as i64),
                            },
                            &txn,
                        )
                        .await?;
                    }
                }
            }
            txn.commit().await?;

            // Publish metrics outside of the txn to reduce txn latency.
            if token_owner_update {
                metric! {
                    statsd_count!("token_account.owner_update", 1);
                }
            }
            if token_delegate_update {
                metric! {
                    statsd_count!("token_account.delegate_update", 1);
                }
            }
            if token_freeze_update {
                metric! {
                    statsd_count!("token_account.freeze_update", 1);
                }
            }

            Ok(())
        }
        TokenProgramAccount::Mint(m) => {
            let freeze_auth: Option<Vec<u8>> = match m.freeze_authority {
                COption::Some(d) => Some(d.to_bytes().to_vec()),
                COption::None => None,
            };
            let mint_auth: Option<Vec<u8>> = match m.mint_authority {
                COption::Some(d) => Some(d.to_bytes().to_vec()),
                COption::None => None,
            };
            let model = tokens::ActiveModel {
                mint: Set(key_bytes.clone()),
                token_program: Set(spl_token_program),
                slot_updated: Set(account_update.slot() as i64),
                supply: Set(m.supply as i64),
                decimals: Set(m.decimals as i32),
                close_authority: Set(None),
                extension_data: Set(None),
                mint_authority: Set(mint_auth),
                freeze_authority: Set(freeze_auth),
                extensions: Set(None),
            };

            let query = token_mint_upsert(model, tokens::Column::ExtensionData);
            db.execute(query).await?;

            let asset_update: Option<asset::Model> = asset::Entity::find_by_id(key_bytes.clone())
                .filter(
                    asset::Column::OwnerType
                        .eq(OwnerType::Single)
                        .or(asset::Column::OwnerType
                            .eq(OwnerType::Unknown)
                            .and(asset::Column::Supply.eq(1))),
                )
                .one(db)
                .await?;

            if asset_update.is_some() {
                upsert_assets_mint_account_columns(
                    AssetMintAccountColumns {
                        mint: key_bytes.clone(),
                        supply_mint: Some(key_bytes.clone()),
                        supply: m.supply as u64,
                        slot_updated_mint_account: account_update.slot(),
                    },
                    db,
                )
                .await?;
            }

            Ok(())
        }
        _ => Err(IngesterError::NotImplemented),
    }?;
    Ok(())
}

#[cfg(test)]
mod upsert_tests;
