use crate::error::IngesterError;
use blockbuster::programs::token_extensions::TokenMetadataAccount;
use digital_asset_types::dao::{asset, asset_data_v2};
use log::{info, warn};
use plerkle_serialization::AccountInfo;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    QueryFilter, QueryTrait,
};

pub async fn handle_token_metadata_account<'a, 'c>(
    tma: &TokenMetadataAccount,
    account_update: &'a AccountInfo<'a>,
    db: &'c DatabaseConnection,
) -> Result<(), IngesterError> {
    let metadata_account_key = *account_update.pubkey().unwrap();
    let metadata_account_bytes = metadata_account_key.0.to_vec();
    let metadata = &tma.metadata;

    info!(
        "Processing TokenMetadata account: {} (name: {}, symbol: {})",
        bs58::encode(&metadata_account_bytes).into_string(),
        metadata.name,
        metadata.symbol
    );

    let mints = find_mints_with_metadata_pointer(db, &metadata_account_bytes).await?;

    if mints.is_empty() {
        info!(
            "No mints found pointing to metadata account {}. This metadata will be linked when the mint is indexed.",
            bs58::encode(&metadata_account_bytes).into_string()
        );
        return Ok(());
    }

    info!(
        "Found {} mint(s) pointing to metadata account {}",
        mints.len(),
        bs58::encode(&metadata_account_bytes).into_string()
    );

    for mint_id in mints {
        match update_asset_data_with_metadata(
            db,
            &mint_id,
            &metadata_account_bytes,
            metadata,
            account_update.slot() as i64,
        )
        .await
        {
            Ok(_) => {
                info!(
                    "Updated asset_data for mint {} with metadata from {}",
                    bs58::encode(&mint_id).into_string(),
                    bs58::encode(&metadata_account_bytes).into_string()
                );
            }
            Err(e) => {
                warn!(
                    "Failed to update asset_data for mint {}: {}",
                    bs58::encode(&mint_id).into_string(),
                    e
                );
            }
        }
    }

    Ok(())
}

/// Find mints that have a metadata_pointer extension pointing to the given metadata account
async fn find_mints_with_metadata_pointer(
    db: &DatabaseConnection,
    metadata_account: &[u8],
) -> Result<Vec<Vec<u8>>, DbErr> {
    let assets: Vec<asset::Model> = asset::Entity::find()
        .filter(asset::Column::T22MetadataAddress.eq(metadata_account))
        .all(db)
        .await?;

    Ok(assets.into_iter().map(|a| a.id).collect())
}

/// Update asset_data_v2 and asset for a mint with metadata from a TokenMetadata account
async fn update_asset_data_with_metadata(
    db: &DatabaseConnection,
    mint_id: &[u8],
    metadata_account_id: &[u8],
    metadata: &blockbuster::programs::token_extensions::extension::ShadowMetadata,
    slot: i64,
) -> Result<(), DbErr> {
    use digital_asset_types::dao::{offchain_metadata, sea_orm_active_enums::{ChainMutability, Mutability}};
    use sea_orm::{sea_query::OnConflict, ActiveValue::Set, Statement};

    // Serialize metadata to JSON
    let metadata_json = serde_json::to_value(metadata.clone())
        .map_err(|e| DbErr::Custom(format!("Failed to serialize metadata: {}", e)))?;

    // Insert offchain_metadata entry if it doesn't exist
    let offchain_metadata_model = offchain_metadata::ActiveModel {
        metadata_url: Set(metadata.uri.trim().to_string()),
        metadata: Set(serde_json::Value::String("processing".to_string())),
        mutability: Set(Mutability::Mutable),
        reindex: Set(true),
        ..Default::default()
    };
    let offchain_metadata_query = offchain_metadata::Entity::insert(offchain_metadata_model)
        .on_conflict(
            OnConflict::columns([offchain_metadata::Column::MetadataUrl])
                .do_nothing()
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    db.execute(offchain_metadata_query).await?;

    // Check if asset_data_v2 already exists for this mint
    let existing = asset_data_v2::Entity::find()
        .filter(asset_data_v2::Column::Id.eq(mint_id))
        .one(db)
        .await?;

    // Only update if this is newer data or if it doesn't exist
    let should_update = match existing {
        Some(ref existing_data) => slot >= existing_data.slot_updated,
        None => true,
    };

    if !should_update {
        info!(
            "Skipping update for mint {} - existing data is newer",
            bs58::encode(mint_id).into_string()
        );
        return Ok(());
    }

    let asset_data_model = asset_data_v2::ActiveModel {
        id: Set(mint_id.to_vec()),
        metadata_url: Set(metadata.uri.trim().to_string()),
        chain_mutability: Set(ChainMutability::Mutable),
        chain_data: Set(metadata_json),
        slot_updated: Set(slot),
        base_info_seq: Set(Some(0)),
        raw_name: Set(Some(metadata.name.clone().into_bytes().to_vec())),
        raw_symbol: Set(Some(metadata.symbol.clone().into_bytes().to_vec())),
    };

    let mut asset_data_query = asset_data_v2::Entity::insert(asset_data_model)
        .on_conflict(
            OnConflict::columns([asset_data_v2::Column::Id])
                .update_columns([
                    asset_data_v2::Column::ChainMutability,
                    asset_data_v2::Column::ChainData,
                    asset_data_v2::Column::MetadataUrl,
                    asset_data_v2::Column::SlotUpdated,
                    asset_data_v2::Column::BaseInfoSeq,
                    asset_data_v2::Column::RawName,
                    asset_data_v2::Column::RawSymbol,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    asset_data_query.sql = format!(
        "{} WHERE excluded.slot_updated >= asset_data_v2.slot_updated",
        asset_data_query.sql
    );

    db.execute(asset_data_query).await?;

    // Also update the asset table to set metadata_account_id so the asset is properly linked
    // This is crucial for the API to recognize this as having metadata
    // NOTE: For external metadata pointers, metadata_account_id is already set to the mint address
    // in mint.rs to avoid unique constraint violations when multiple mints share a container.
    // We only update if metadata_account_id is NULL (meaning the mint was indexed before the fix).
    let update_asset_sql = format!(
        "UPDATE asset SET metadata_account_id = $1 WHERE id = $2 AND metadata_account_id IS NULL"
    );

    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        &update_asset_sql,
        vec![metadata_account_id.into(), mint_id.into()],
    ))
    .await?;

    Ok(())
}
