use crate::program_transformers::asset_upserts::{
    upsert_assets_metadata_account_columns, upsert_assets_mint_account_columns,
    upsert_assets_token_account_columns, AssetMetadataAccountColumns, AssetMintAccountColumns,
    AssetTokenAccountColumns,
};
use crate::tasks::{DownloadMetadata, IntoTaskData};
use crate::{error::IngesterError, metric, tasks::TaskData};
use blockbuster::token_metadata::{
    accounts::{MasterEdition, Metadata},
    types::TokenStandard,
};
use cadence_macros::{is_global_default_set, statsd_count};
use chrono::Utc;
use digital_asset_types::dao::{asset_authority, asset_data, asset_grouping, token_accounts};
use digital_asset_types::{
    dao::{
        asset, asset_creators, asset_v1_account_attachments,
        sea_orm_active_enums::{
            ChainMutability, Mutability, OwnerType, SpecificationAssetClass, SpecificationVersions,
            V1AccountAttachments,
        },
        tokens,
    },
    json::ChainDataV1,
};
use lazy_static::lazy_static;
use log::warn;
use plerkle_serialization::Pubkey as FBPubkey;
use sea_orm::{
    entity::*, query::*, sea_query::OnConflict, ActiveValue::Set, ConnectionTrait, DbBackend,
    DbErr, EntityTrait, JsonValue,
};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::time::Duration;
use tokio::time::sleep;

pub async fn burn_v1_asset<T: ConnectionTrait + TransactionTrait>(
    conn: &T,
    id: FBPubkey,
    slot: u64,
) -> Result<(), IngesterError> {
    let (id, slot_i) = (id.0, slot as i64);
    let model = asset::ActiveModel {
        id: Set(id.to_vec()),
        slot_updated: Set(Some(slot_i)),
        burnt: Set(true),
        ..Default::default()
    };
    let mut query = asset::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns([asset::Column::SlotUpdated, asset::Column::Burnt])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    query.sql = format!(
        "{} WHERE excluded.slot_updated > asset.slot_updated",
        query.sql
    );
    conn.execute(query).await?;
    Ok(())
}

const RETRY_INTERVALS: &[u64] = &[0, 5, 10];
const WSOL_ADDRESS: &str = "So11111111111111111111111111111111111111112";

lazy_static! {
    static ref WSOL_PUBKEY: Pubkey =
        Pubkey::from_str(WSOL_ADDRESS).expect("Invalid public key format");
}

pub async fn index_and_fetch_mint_data<T: ConnectionTrait + TransactionTrait>(
    conn: &T,
    mint_pubkey_vec: Vec<u8>,
) -> Result<Option<tokens::Model>, IngesterError> {
    // Gets the token and token account for the mint to populate the asset.
    // This is required when the token and token account are indexed, but not the metadata account.
    // If the metadata account is indexed, then the token and ta ingester will update the asset with the correct data.
    let token: Option<tokens::Model> = find_model_with_retry(
        conn,
        "token",
        &tokens::Entity::find_by_id(mint_pubkey_vec.clone()),
        RETRY_INTERVALS,
    )
    .await?;

    if let Some(token) = token {
        upsert_assets_mint_account_columns(
            AssetMintAccountColumns {
                mint: mint_pubkey_vec.clone(),
                suppply_mint: Some(token.mint.clone()),
                supply: token.supply as u64,
                slot_updated_mint_account: token.slot_updated as u64,
            },
            conn,
        )
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
        Ok(Some(token))
    } else {
        warn!(
            target: "Mint not found",
            "Mint not found in 'tokens' table for mint {}",
            bs58::encode(&mint_pubkey_vec).into_string()
        );
        Ok(None)
    }
}

async fn index_token_account_data<T: ConnectionTrait + TransactionTrait>(
    conn: &T,
    mint_pubkey_vec: Vec<u8>,
) -> Result<(), IngesterError> {
    let token_account: Option<token_accounts::Model> = find_model_with_retry(
        conn,
        "owners",
        &token_accounts::Entity::find()
            .filter(token_accounts::Column::Mint.eq(mint_pubkey_vec.clone()))
            .filter(token_accounts::Column::Amount.gt(0))
            .order_by(token_accounts::Column::SlotUpdated, Order::Desc),
        RETRY_INTERVALS,
    )
    .await
    .map_err(|e: DbErr| IngesterError::DatabaseError(e.to_string()))?;

    if let Some(token_account) = token_account {
        upsert_assets_token_account_columns(
            AssetTokenAccountColumns {
                mint: mint_pubkey_vec.clone(),
                owner: Some(token_account.owner),
                delegate: token_account.delegate,
                frozen: token_account.frozen,
                slot_updated_token_account: Some(token_account.slot_updated),
            },
            conn,
        )
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    } else {
        warn!(
            target: "Account not found",
            "Token acc not found in 'owners' table for mint {}",
            bs58::encode(&mint_pubkey_vec).into_string()
        );
    }

    Ok(())
}

pub async fn save_v1_asset<T: ConnectionTrait + TransactionTrait>(
    conn: &T,
    metadata: &Metadata,
    slot: u64,
) -> Result<Option<TaskData>, IngesterError> {
    let metadata = metadata.clone();
    let mint_pubkey = metadata.mint;
    let mint_pubkey_array = mint_pubkey.to_bytes();
    let mint_pubkey_vec = mint_pubkey_array.to_vec();

    let (edition_attachment_address, _) = MasterEdition::find_pda(&mint_pubkey);

    let authority = metadata.update_authority.to_bytes().to_vec();
    let slot_i = slot as i64;
    let uri = metadata.uri.trim().replace('\0', "");
    let _spec = SpecificationVersions::V1;
    let mut class = match metadata.token_standard {
        Some(TokenStandard::NonFungible) => SpecificationAssetClass::Nft,
        Some(TokenStandard::FungibleAsset) => SpecificationAssetClass::FungibleAsset,
        Some(TokenStandard::Fungible) => SpecificationAssetClass::FungibleToken,
        Some(TokenStandard::NonFungibleEdition) => SpecificationAssetClass::Nft,
        Some(TokenStandard::ProgrammableNonFungible) => SpecificationAssetClass::ProgrammableNft,
        Some(TokenStandard::ProgrammableNonFungibleEdition) => {
            SpecificationAssetClass::ProgrammableNft
        }
        _ => SpecificationAssetClass::Unknown,
    };
    let mut ownership_type = match class {
        SpecificationAssetClass::FungibleAsset => OwnerType::Token,
        SpecificationAssetClass::FungibleToken => OwnerType::Token,
        SpecificationAssetClass::Nft | SpecificationAssetClass::ProgrammableNft => {
            OwnerType::Single
        }
        _ => OwnerType::Unknown,
    };

    // Wrapped Solana is a special token that has supply 0 (infinite).
    // It's a fungible token with a metadata account, but without any token standard, meaning the code above will misabel it as an NFT.
    if mint_pubkey == *WSOL_PUBKEY {
        ownership_type = OwnerType::Token;
        class = SpecificationAssetClass::FungibleToken;
    }

    let token: Option<tokens::Model> =
        index_and_fetch_mint_data(conn, mint_pubkey_vec.clone()).await?;

    // get supply of token, default to 1 since most cases will be NFTs. Token mint ingester will properly set supply if token_result is None
    let supply = token.map(|t| t.supply).unwrap_or(1);

    // Map unknown ownership types based on the supply.
    if ownership_type == OwnerType::Unknown {
        ownership_type = match supply.cmp(&1) {
            std::cmp::Ordering::Equal => OwnerType::Single,
            std::cmp::Ordering::Greater => OwnerType::Token,
            _ => OwnerType::Unknown,
        };
    };

    if (ownership_type == OwnerType::Single) | (ownership_type == OwnerType::Unknown) {
        index_token_account_data(conn, mint_pubkey_vec.clone()).await?;
    }

    let name = metadata.name.clone().into_bytes();
    let symbol = metadata.symbol.clone().into_bytes();
    let mut chain_data = ChainDataV1 {
        name: metadata.name.clone(),
        symbol: metadata.symbol.clone(),
        edition_nonce: metadata.edition_nonce,
        primary_sale_happened: metadata.primary_sale_happened,
        token_standard: metadata.token_standard,
        uses: metadata.uses,
    };
    chain_data.sanitize();
    let chain_data_json = serde_json::to_value(chain_data)
        .map_err(|e| IngesterError::DeserializationError(e.to_string()))?;
    let chain_mutability = match metadata.is_mutable {
        true => ChainMutability::Mutable,
        false => ChainMutability::Immutable,
    };
    let asset_data_model = asset_data::ActiveModel {
        chain_data_mutability: Set(chain_mutability),
        chain_data: Set(chain_data_json),
        metadata_url: Set(uri.clone()),
        metadata: Set(JsonValue::String("processing".to_string())),
        metadata_mutability: Set(Mutability::Mutable),
        slot_updated: Set(slot_i),
        reindex: Set(Some(true)),
        id: Set(mint_pubkey_vec.clone()),
        raw_name: Set(Some(name.to_vec())),
        raw_symbol: Set(Some(symbol.to_vec())),
        base_info_seq: Set(Some(0)),
    };
    let txn = conn.begin().await?;
    let mut query = asset_data::Entity::insert(asset_data_model)
        .on_conflict(
            OnConflict::columns([asset_data::Column::Id])
                .update_columns([
                    asset_data::Column::ChainDataMutability,
                    asset_data::Column::ChainData,
                    asset_data::Column::MetadataUrl,
                    asset_data::Column::MetadataMutability,
                    asset_data::Column::SlotUpdated,
                    asset_data::Column::Reindex,
                    asset_data::Column::RawName,
                    asset_data::Column::RawSymbol,
                    asset_data::Column::BaseInfoSeq,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    query.sql = format!(
        "{} WHERE excluded.slot_updated > asset_data.slot_updated",
        query.sql
    );
    txn.execute(query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    upsert_assets_metadata_account_columns(
        AssetMetadataAccountColumns {
            mint: mint_pubkey_vec.clone(),
            owner_type: ownership_type,
            specification_asset_class: Some(class),
            royalty_amount: metadata.seller_fee_basis_points as i32,
            asset_data: Some(mint_pubkey_vec.clone()),
            slot_updated_metadata_account: slot_i as u64,
        },
        &txn,
    )
    .await?;

    let attachment = asset_v1_account_attachments::ActiveModel {
        id: Set(edition_attachment_address.to_bytes().to_vec()),
        slot_updated: Set(slot_i),
        attachment_type: Set(V1AccountAttachments::MasterEditionV2),
        ..Default::default()
    };
    let query = asset_v1_account_attachments::Entity::insert(attachment)
        .on_conflict(
            OnConflict::columns([asset_v1_account_attachments::Column::Id])
                .do_nothing()
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    txn.execute(query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    let model = asset_authority::ActiveModel {
        asset_id: Set(mint_pubkey_vec.clone()),
        authority: Set(authority),
        seq: Set(0),
        slot_updated: Set(slot_i),
        ..Default::default()
    };
    let mut query = asset_authority::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([asset_authority::Column::AssetId])
                .update_columns([
                    asset_authority::Column::Authority,
                    asset_authority::Column::Seq,
                    asset_authority::Column::SlotUpdated,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    query.sql = format!(
        "{} WHERE excluded.slot_updated > asset_authority.slot_updated",
        query.sql
    );
    txn.execute(query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    if let Some(c) = &metadata.collection {
        let model = asset_grouping::ActiveModel {
            asset_id: Set(mint_pubkey_vec.clone()),
            group_key: Set("collection".to_string()),
            group_value: Set(Some(c.key.to_string())),
            verified: Set(c.verified),
            group_info_seq: Set(Some(0)),
            slot_updated: Set(Some(slot_i)),
            ..Default::default()
        };
        let mut query = asset_grouping::Entity::insert(model)
            .on_conflict(
                OnConflict::columns([
                    asset_grouping::Column::AssetId,
                    asset_grouping::Column::GroupKey,
                ])
                .update_columns([
                    asset_grouping::Column::GroupValue,
                    asset_grouping::Column::Verified,
                    asset_grouping::Column::SlotUpdated,
                    asset_grouping::Column::GroupInfoSeq,
                ])
                .to_owned(),
            )
            .build(DbBackend::Postgres);
        query.sql = format!(
            "{} WHERE excluded.slot_updated > asset_grouping.slot_updated",
            query.sql
        );
        txn.execute(query)
            .await
            .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    }

    let creators = metadata
        .creators
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, creator)| asset_creators::ActiveModel {
            asset_id: Set(mint_pubkey_vec.clone()),
            position: Set(i as i16),
            creator: Set(creator.address.to_bytes().to_vec()),
            share: Set(creator.share as i32),
            verified: Set(creator.verified),
            slot_updated: Set(Some(slot_i)),
            seq: Set(Some(0)),
            ..Default::default()
        })
        .collect::<Vec<_>>();

    if !creators.is_empty() {
        let mut query = asset_creators::Entity::insert_many(creators)
            .on_conflict(
                OnConflict::columns([
                    asset_creators::Column::AssetId,
                    asset_creators::Column::Position,
                ])
                .update_columns([
                    asset_creators::Column::Creator,
                    asset_creators::Column::Share,
                    asset_creators::Column::Verified,
                    asset_creators::Column::Seq,
                    asset_creators::Column::SlotUpdated,
                ])
                .to_owned(),
            )
            .build(DbBackend::Postgres);
        query.sql = format!(
                "{} WHERE excluded.slot_updated >= asset_creators.slot_updated OR asset_creators.slot_updated is NULL",
                query.sql
            );
        txn.execute(query)
            .await
            .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    }
    txn.commit().await?;

    if uri.is_empty() {
        warn!(
            "URI is empty for mint {}. Skipping background task.",
            bs58::encode(mint_pubkey_vec).into_string()
        );
        return Ok(None);
    }

    let mut task = DownloadMetadata {
        asset_data_id: mint_pubkey_vec.clone(),
        uri,
        created_at: Some(Utc::now().naive_utc()),
    };
    task.sanitize();
    let t = task.into_task_data()?;
    Ok(Some(t))
}

async fn find_model_with_retry<T: ConnectionTrait + TransactionTrait, K: EntityTrait>(
    conn: &T,
    model_name: &str,
    select: &Select<K>,
    retry_intervals: &[u64],
) -> Result<Option<K::Model>, DbErr> {
    let mut retries = 0;
    let metric_name = format!("{}_found", model_name);

    for interval in retry_intervals {
        let interval_duration = Duration::from_millis(interval.to_owned());
        sleep(interval_duration).await;

        let model = select.clone().one(conn).await?;
        if let Some(m) = model {
            record_metric(&metric_name, true, retries);
            return Ok(Some(m));
        }
        retries += 1;
    }

    record_metric(&metric_name, false, retries - 1);
    Ok(None)
}

fn record_metric(metric_name: &str, success: bool, retries: u32) {
    let retry_count = &retries.to_string();
    let success = if success { "true" } else { "false" };
    metric! {
        statsd_count!(metric_name, 1, "success" => success, "retry_count" => retry_count);
    }
}
