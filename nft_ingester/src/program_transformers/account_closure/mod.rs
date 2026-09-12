use crate::{config::IngesterConfig, error::IngesterError, metric, tasks::TaskData};
use blockbuster::programs::account_closure::AccountClosureData;
use cadence_macros::{is_global_default_set, statsd_count, statsd_time};
use digital_asset_types::dao::{asset, owners};
use log::info;
use mpl_token_metadata::accounts::Metadata;
use plerkle_serialization::AccountInfo;
use sea_orm::{
    entity::*, query::*, sea_query::OnConflict, ActiveValue::Set, DatabaseConnection, DbBackend,
    DbErr, EntityTrait,
};
use solana_sdk::pubkey::Pubkey;
use sqlx::types::Decimal;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

pub async fn handle_account_closure<'a, 'b, 'c>(
    account_update: &'a AccountInfo<'a>,
    parsing_result: &'b AccountClosureData,
    db: &'c DatabaseConnection,
    _task_manager: &UnboundedSender<TaskData>,
    _config: &IngesterConfig,
) -> Result<(), IngesterError> {
    let key = *account_update.pubkey().unwrap();
    let key_bytes = key.0.to_vec();
    match &parsing_result {
        AccountClosureData::ClosedAccountInfo(_data) => {
            match parse_account_type(db, key_bytes).await? {
                AccountType::TokenAccount(token_account) => {
                    if !token_account.closed.unwrap_or(false)
                        || (token_account.token_amount.unwrap_or(0) > 0)
                        || (token_account.token_amount_u64.unwrap_or(Decimal::ZERO) > Decimal::ZERO)
                    {
                        mark_token_account_as_closed(
                            token_account,
                            db,
                            account_update.slot() as u64,
                        )
                        .await?;
                    }
                }
                AccountType::NFT(nft) => {
                    mark_nft_as_burnt(nft, db, account_update.slot() as u64).await?;
                }
                AccountType::Unknown => {}
            }
        }
        AccountClosureData::EmptyAccount => {}
    };
    Ok(())
}

#[derive(Debug)]
pub enum AccountType {
    NFT(asset::Model),
    TokenAccount(owners::Model),
    Unknown,
}

pub async fn parse_account_type(
    db: &DatabaseConnection,
    key_bytes: Vec<u8>,
) -> Result<AccountType, IngesterError> {
    let token_account = owners::Entity::find()
        .filter(owners::Column::TokenAccount.eq(key_bytes.clone()))
        .one(db)
        .await?;
    if let Some(token_account) = token_account {
        return Ok(AccountType::TokenAccount(token_account));
    }

    let nft: Option<asset::Model> = asset::Entity::find()
        .filter(asset::Column::MetadataAccountId.eq(key_bytes.clone()))
        .one(db)
        .await?;
    if let Some(nft) = nft {
        return Ok(AccountType::NFT(nft));
    }

    Ok(AccountType::Unknown)
}

pub async fn mark_token_account_as_closed(
    token_account: owners::Model,
    db: &DatabaseConnection,
    slot: u64,
) -> Result<(), DbErr> {
    let mut owners_model: owners::ActiveModel = token_account.clone().into();
    owners_model.closed = Set(Some(true));
    owners_model.token_amount = Set(Some(0));
    owners_model.token_amount_u64 = Set(Some(Decimal::ZERO));
    owners_model.slot_updated = Set(Some(slot as i64));
    let mut query = owners::Entity::insert(owners_model)
        .on_conflict(
            OnConflict::column(owners::Column::TokenAccount)
                .update_columns([
                    owners::Column::TokenAmount,
                    owners::Column::TokenAmountU64,
                    owners::Column::Closed,
                    owners::Column::SlotUpdated,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    query.sql = format!(
        "{} WHERE excluded.slot_updated >= owners.slot_updated OR owners.slot_updated IS NULL",
        query.sql
    );
    let start = Instant::now();
    db.execute(query).await?;
    metric! {
        statsd_time!("token_account.closed_latency", start.elapsed());
    }
    metric! {
        statsd_count!("token_account.closed", 1);
    }
    Ok(())
}

pub async fn mark_nft_as_burnt(
    nft: asset::Model,
    db: &DatabaseConnection,
    slot: u64,
) -> Result<(), DbErr> {
    let id = Pubkey::try_from(nft.id.clone()).expect("Error parsing NFT id");
    let metadata_account_id = Metadata::find_pda(&id).0;
    info!(
        "Marking the following NFT with id {} and metadata account id {} as burnt.",
        id, metadata_account_id
    );
    let mut nft: asset::ActiveModel = nft.into();
    nft.burnt = Set(true);
    nft.slot_updated_metadata_account = Set(Some(slot as i64));
    // Max slot updated is auto-generated
    nft.slot_updated = NotSet;

    let mut query = asset::Entity::insert(nft)
        .on_conflict(
            OnConflict::column(asset::Column::Id)
                .update_columns([
                    asset::Column::Burnt,
                    asset::Column::SlotUpdatedMetadataAccount,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    query.sql = format!(
        "{} WHERE excluded.slot_updated_metadata_account >= asset.slot_updated_metadata_account OR asset.slot_updated_metadata_account IS NULL",
        query.sql
    );
    let start = Instant::now();
    db.execute(query).await?;
    metric! {
        statsd_time!("nft.burn_latency", start.elapsed());
    }
    metric! {
        statsd_count!("nft.burnt", 1);
    }
    Ok(())
}
