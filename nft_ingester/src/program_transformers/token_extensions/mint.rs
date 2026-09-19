use crate::{
    error::IngesterError,
    program_transformers::{
        asset_upserts::{
            download_task_warranted, guard_asset_data_v2_noop, guard_offchain_insert_repair,
            upsert_assets_token_account_columns, AssetTokenAccountColumns,
        },
        utils::find_model_with_retry,
    },
    tasks::{DownloadMetadata, IntoTaskData, TaskData},
};
use blockbuster::programs::token_extensions::{
    extension::ShadowMetadata, MintAccount, MintAccountExtensions,
};
use cadence_macros::statsd_count;
use chrono::Utc;
use common::metric;
use digital_asset_types::dao::{
    asset, asset_data_v2, offchain_metadata, owners,
    sea_orm_active_enums::{
        ChainMutability, Mutability, OwnerType, SpecificationAssetClass, SpecificationVersions,
    },
    tokens, AuthorityInfo,
};
use log::warn;
use plerkle_serialization::AccountInfo;
use sea_orm::{
    entity::*, query::*, sea_query::OnConflict, ActiveValue::Set, ConnectionTrait,
    DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, EntityTrait,
};
use solana_sdk::{program_option::COption, pubkey::Pubkey};

const RETRY_INTERVALS: &[u64] = &[0, 5, 10];

// Helper function to convert OptionalNonZeroPubkey to Option<Pubkey>
// OptionalNonZeroPubkey uses all-zeros to represent None
fn optional_pubkey_to_option(
    opt_pubkey: spl_pod::optional_keys::OptionalNonZeroPubkey,
) -> Option<Pubkey> {
    let bytes: &[u8; 32] = bytemuck::cast_ref(&opt_pubkey);
    if bytes.iter().all(|&b| b == 0) {
        None
    } else {
        Some(Pubkey::new_from_array(*bytes))
    }
}

pub async fn handle_token_extensions_mint_account<'a, 'b, 'c>(
    m: &MintAccount,
    account_update: &'a AccountInfo<'a>,
    db: &'c DatabaseConnection,
) -> Result<Option<TaskData>, IngesterError> {
    let key = *account_update.pubkey().unwrap();
    let key_bytes = key.0.to_vec();
    let spl_token_program = account_update.owner().unwrap().0.to_vec();

    let mut task: Option<TaskData> = None;

    let sanitized_extensions = {
        let mut extension = m.extensions.clone();
        extension.sanitize();
        extension
    };

    let txn = db.begin().await?;

    insert_into_tokens_table(
        m,
        key_bytes.clone(),
        spl_token_program,
        account_update.slot() as i64,
        &sanitized_extensions,
        &txn,
    )
    .await?;

    let metadata_to_use = if let Some(metadata) = sanitized_extensions.metadata.clone() {
        Some(metadata)
    } else if let Some(metadata_pointer) = &sanitized_extensions.metadata_pointer {
        let metadata_addr_opt: Option<Pubkey> =
            optional_pubkey_to_option(metadata_pointer.metadata_address);
        if let Some(metadata_addr) = metadata_addr_opt {
            let metadata_addr_bytes = metadata_addr.to_bytes().to_vec();
            let metadata_asset_data = asset_data_v2::Entity::find_by_id(metadata_addr_bytes)
                .one(&txn)
                .await?;

            if let Some(metadata_data) = metadata_asset_data {
                serde_json::from_value::<ShadowMetadata>(metadata_data.chain_data).ok()
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Store metadata and whether we need to update pointing mints
    let metadata_for_pointing_mints = if let Some(metadata) = metadata_to_use {
        // Check if metadata changed BEFORE updating, to decide if we need to update pointing mints
        // This must happen before the transaction commits to avoid race conditions with new mints
        let should_update_pointing_mints = if sanitized_extensions.metadata.is_some() {
            // This mint has inline metadata, so it's a potential container
            // Check if the metadata actually changed
            let old_metadata_data = asset_data_v2::Entity::find_by_id(key_bytes.clone())
                .one(&txn)
                .await?;

            match old_metadata_data {
                None => {
                    // No existing metadata, this is first time insert
                    true
                }
                Some(old_data) => {
                    // Compare old and new metadata
                    match serde_json::from_value::<ShadowMetadata>(old_data.chain_data) {
                        Ok(old_metadata) => {
                            // Check if any relevant metadata fields changed
                            old_metadata.name != metadata.name
                                || old_metadata.symbol != metadata.symbol
                                || old_metadata.uri != metadata.uri
                                || old_metadata.update_authority != metadata.update_authority
                                || old_metadata.additional_metadata != metadata.additional_metadata
                        }
                        Err(_) => {
                            // Can't parse old metadata, assume changed
                            true
                        }
                    }
                }
            }
        } else {
            // This mint doesn't have inline metadata, so no pointing mints to update
            false
        };

        let offchain_rows = insert_offchain_metadata(&metadata, &txn).await?;

        let asset_data_rows = upsert_asset_data_v2(
            &metadata,
            &metadata,
            key_bytes.clone(),
            account_update.slot() as i64,
            &txn,
        )
        .await?;

        // A mint touch that neither introduced a new URI nor changed stored
        // metadata is no evidence the off-chain document changed — skip the task.
        if download_task_warranted(offchain_rows, asset_data_rows) {
            task = Some(create_task(&metadata, key_bytes.clone())?);
        } else {
            metric! {
                statsd_count!("ingester.bgtask.noop_metadata_skip", 1);
            }
        }

        // Return metadata only if we determined it changed and we need to update pointing mints
        if should_update_pointing_mints {
            Some(metadata)
        } else {
            None
        }
    } else {
        None
    };

    if should_upsert_asset(m) {
        upsert_asset(
            m,
            key_bytes.clone(),
            account_update.slot() as i64,
            db,
            &sanitized_extensions,
            &txn,
        )
        .await?;
    }

    // Commit the main transaction first to release locks quickly
    txn.commit().await?;

    // If this mint has inline metadata that changed, update asset_data_v2 for any mints pointing to it via metadata_pointer
    // This is done in a separate transaction to avoid holding locks during the pointing mints query/update
    // This handles the case where this mint is a metadata container (e.g., supply=0) that other mints reference
    // metadata_for_pointing_mints is only Some if metadata actually changed (checked before txn commit above)
    if let Some(metadata) = metadata_for_pointing_mints {
        use digital_asset_types::dao::asset;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

        // Metadata changed, update all pointing mints
        // Start a new transaction for pointing mints update
        let pointing_txn = db.begin().await?;

        let mints_pointing_here: Vec<asset::Model> = asset::Entity::find()
            .filter(asset::Column::T22MetadataAddress.eq(key_bytes.clone()))
            .all(&pointing_txn)
            .await?;

        log::info!(
            "Metadata changed for container {}, updating {} pointing mints",
            bs58::encode(&key_bytes).into_string(),
            mints_pointing_here.len()
        );

        for pointing_mint in mints_pointing_here {
            // If individual updates fail, log but continue with eventual consistency
            if let Err(e) = upsert_asset_data_v2(
                &metadata,
                &metadata,
                pointing_mint.id.clone(),
                account_update.slot() as i64,
                &pointing_txn,
            )
            .await
            {
                log::warn!(
                    "Failed to update pointing mint {} for container {}: {}",
                    bs58::encode(&pointing_mint.id).into_string(),
                    bs58::encode(&key_bytes).into_string(),
                    e
                );
            }
        }

        // Commit the pointing mints transaction
        // If this fails, the container mint is still updated (eventual consistency)
        if let Err(e) = pointing_txn.commit().await {
            log::error!(
                "Failed to commit pointing mints transaction for container {}: {}",
                bs58::encode(&key_bytes).into_string(),
                e
            );
        }
    }

    Ok(task)
}

// A mint with no metadata extension only becomes an asset once it is a single-supply NFT.
fn should_upsert_asset(m: &MintAccount) -> bool {
    is_token_nft(m) || m.extensions.metadata.is_some() || m.extensions.metadata_pointer.is_some()
}

fn is_token_nft(m: &MintAccount) -> bool {
    m.account.supply == 1 && m.account.decimals == 0
}

async fn insert_into_tokens_table(
    m: &MintAccount,
    key_bytes: Vec<u8>,
    spl_token_program: Vec<u8>,
    slot: i64,
    sanitized_extensions: &MintAccountExtensions,
    txn: &DatabaseTransaction,
) -> Result<(), IngesterError> {
    let extensions = serde_json::to_value(sanitized_extensions)
        .map_err(|e| IngesterError::SerializatonError(e.to_string()))?;
    let freeze_auth: Option<Vec<u8>> = match m.account.freeze_authority {
        COption::Some(d) => Some(d.to_bytes().to_vec()),
        COption::None => None,
    };
    let mint_auth: Option<Vec<u8>> = match m.account.mint_authority {
        COption::Some(d) => Some(d.to_bytes().to_vec()),
        COption::None => None,
    };
    let tokens_model = tokens::ActiveModel {
        mint: Set(key_bytes.clone()),
        token_program: Set(spl_token_program),
        slot_updated: Set(slot),
        supply: Set(m.account.supply as i64),
        decimals: Set(m.account.decimals as i32),
        close_authority: Set(None),
        extension_data: Set(None),
        mint_authority: Set(mint_auth),
        freeze_authority: Set(freeze_auth),
        extensions: Set(Some(extensions.clone())),
    };

    let tokens_query =
        super::super::token::token_mint_upsert(tokens_model, tokens::Column::Extensions);

    txn.execute(tokens_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    Ok(())
}

/// Returns the number of rows written (zero when the URI is already stored).
async fn insert_offchain_metadata(
    metadata: &ShadowMetadata,
    txn: &DatabaseTransaction,
) -> Result<u64, IngesterError> {
    let offchain_metadata_model = offchain_metadata::ActiveModel {
        metadata_url: Set(metadata.uri.trim().to_string()),
        metadata: Set(JsonValue::String("processing".to_string())),
        mutability: Set(Mutability::Mutable),
        reindex: Set(true),
        ..Default::default()
    };
    let mut offchain_metadata_query = offchain_metadata::Entity::insert(offchain_metadata_model)
        .on_conflict(
            OnConflict::columns([offchain_metadata::Column::MetadataUrl])
                .update_columns([offchain_metadata::Column::Reindex])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    offchain_metadata_query.sql = guard_offchain_insert_repair(offchain_metadata_query.sql);
    let res = txn
        .execute(offchain_metadata_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    Ok(res.rows_affected())
}

/// Returns the number of rows written (zero when nothing stored would change).
async fn upsert_asset_data_v2(
    metadata: &ShadowMetadata,
    sanitized_metadata: &ShadowMetadata,
    key_bytes: Vec<u8>,
    slot: i64,
    txn: &DatabaseTransaction,
) -> Result<u64, IngesterError> {
    let metadata_json = serde_json::to_value(sanitized_metadata.clone())
        .map_err(|e| IngesterError::SerializatonError(e.to_string()))?;
    let asset_data_model = asset_data_v2::ActiveModel {
        metadata_url: Set(sanitized_metadata.uri.trim().to_string()),
        id: Set(key_bytes.clone()),
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
    asset_data_query.sql = guard_asset_data_v2_noop(asset_data_query.sql);
    let res = txn
        .execute(asset_data_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    Ok(res.rows_affected())
}

async fn upsert_asset(
    m: &MintAccount,
    key_bytes: Vec<u8>,
    slot: i64,
    db: &DatabaseConnection,
    sanitized_extensions: &MintAccountExtensions,
    txn: &DatabaseTransaction,
) -> Result<(), IngesterError> {
    let is_nft = is_token_nft(m);
    let owner_type = match is_nft {
        true => OwnerType::Single,
        false => OwnerType::Token,
    };
    if is_nft {
        let token_account: Option<owners::Model> = find_model_with_retry(
            db,
            "owners",
            &owners::Entity::find()
                .filter(owners::Column::Mint.eq(key_bytes.clone()))
                .filter(owners::Column::TokenAmount.gt(0))
                .order_by(owners::Column::SlotUpdated, Order::Desc),
            RETRY_INTERVALS,
        )
        .await
        .map_err(|e: DbErr| IngesterError::DatabaseError(e.to_string()))?;

        match token_account {
            Some(ta) => {
                upsert_assets_token_account_columns(
                    AssetTokenAccountColumns {
                        mint: key_bytes.clone(),
                        owner: ta.owner,
                        frozen: ta.frozen,
                        delegate: ta.delegate,
                        token_extensions: ta.token_extensions,
                        slot_updated_token_account: ta.slot_updated,
                    },
                    txn,
                )
                .await?
            }
            None => {
                if m.account.supply == 1 {
                    warn!(
                        target: "Account not found",
                        "Token acc not found in 'owners' table for mint {}",
                        bs58::encode(&key_bytes).into_string()
                    );
                }
            }
        }
    }

    let extensions = serde_json::to_value(sanitized_extensions)
        .map_err(|e| IngesterError::SerializatonError(e.to_string()))?;

    let class = match is_nft {
        true => SpecificationAssetClass::Nft,
        false => SpecificationAssetClass::FungibleToken,
    };

    let t22_metadata_addr = sanitized_extensions
        .metadata_pointer
        .as_ref()
        .and_then(|mp| {
            let addr_opt: Option<Pubkey> = optional_pubkey_to_option(mp.metadata_address);
            addr_opt.map(|addr| addr.to_bytes().to_vec())
        });

    // Determine metadata_account_id based on whether metadata is inline or external
    let metadata_account_id = if sanitized_extensions.metadata.is_some() {
        // Inline metadata: metadata_account_id is the mint itself
        Some(key_bytes.clone())
    } else if t22_metadata_addr.is_some() {
        // External metadata via pointer: use the mint address to avoid unique constraint violations
        // Multiple mints can point to the same external metadata container
        Some(key_bytes.clone())
    } else {
        // No metadata
        None
    };

    let mut asset_model = asset::ActiveModel {
        id: Set(key_bytes.clone()),
        owner_type: Set(owner_type),
        supply: Set(m.account.supply as i64),
        supply_mint: Set(Some(key_bytes.clone())),
        specification_version: Set(Some(SpecificationVersions::V1)),
        specification_asset_class: Set(Some(class)),
        nonce: Set(Some(0)),
        seq: Set(Some(0)),
        compressed: Set(false),
        compressible: Set(false),
        asset_data: Set(Some(key_bytes.clone())),
        slot_updated_mint_account: Set(Some(slot)),
        burnt: Set(false),
        mint_extensions: Set(Some(extensions)),
        metadata_account_id: Set(metadata_account_id),
        t22_metadata_address: Set(t22_metadata_addr),
        ..Default::default()
    };

    let auth_address: Option<Vec<u8>> = sanitized_extensions.metadata.clone().and_then(|m| {
        let auth_pubkey: Option<Pubkey> = optional_pubkey_to_option(m.update_authority);
        auth_pubkey.map(|value| value.to_bytes().to_vec())
    });
    if let Some(authority) = auth_address {
        asset_model.authority_address = Set(Some(authority.clone()));
        asset_model.authority_seq = Set(Some(0));
        asset_model.authority_slot_updated = Set(Some(slot));
        asset_model.authority_scopes = Set(Some(vec!["metadata".to_string()]));
        let authority_info = Some(AuthorityInfo {
            authority,
            seq: 0,
            slot_updated: slot,
            scopes: Some(vec!["metadata".to_string()]),
        });
        asset_model.authorities_info = Set(Some(authority_info.into()));
    }

    // Populate collections_info for Token Group Member NFTs
    // This enables searchAssets by collection/grouping for Token-2022 group members
    if let Some(token_group_member) = &sanitized_extensions.token_group_member {
        use digital_asset_types::dao::CollectionsInfo;

        let group_address = bs58::encode(&token_group_member.group).into_string();
        let collections_info = CollectionsInfo {
            collection_id: Some(group_address),
            verified: true, // Token Group membership is verified on-chain
            seq: None,
            slot_updated: slot,
            collection_info_seq: None,
            collection_nft: None,
            collection_size: None, // Size is tracked in the group mint, not individual members
            groups: Vec::new(),
        };
        asset_model.collections_info = Set(Some(collections_info.into()));
    }

    let mut asset_query = asset::Entity::insert(asset_model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns(mint_update_columns(m))
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    asset_query.sql = format!(
        "{} WHERE excluded.slot_updated_mint_account >= asset.slot_updated_mint_account OR asset.slot_updated_mint_account IS NULL",
        asset_query.sql
    );
    txn.execute(asset_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    Ok(())
}

// `burnt` is owned by the account-closure handler; a mint update never clears it.
// A zero-supply update keeps the class an earlier non-zero supply established.
fn mint_update_columns(m: &MintAccount) -> Vec<asset::Column> {
    let mut columns = vec![
        asset::Column::Supply,
        asset::Column::SupplyMint,
        asset::Column::SpecificationVersion,
        asset::Column::Nonce,
        asset::Column::Seq,
        asset::Column::Compressed,
        asset::Column::Compressible,
        asset::Column::AssetData,
        asset::Column::SlotUpdatedMintAccount,
        asset::Column::AuthorityAddress,
        asset::Column::AuthoritySeq,
        asset::Column::AuthoritySlotUpdated,
        asset::Column::AuthorityScopes,
        asset::Column::AuthoritiesInfo,
        asset::Column::MintExtensions,
        asset::Column::MetadataAccountId,
        asset::Column::T22MetadataAddress,
        asset::Column::CollectionsInfo,
    ];
    if m.account.supply > 0 {
        columns.push(asset::Column::OwnerType);
        columns.push(asset::Column::SpecificationAssetClass);
    }
    columns
}

fn create_task(metadata: &ShadowMetadata, key_bytes: Vec<u8>) -> Result<TaskData, IngesterError> {
    let mut dm = DownloadMetadata {
        asset_data_id: key_bytes.clone(),
        uri: metadata.uri.trim().to_string(),
        created_at: Some(Utc::now().naive_utc()),
    };
    dm.sanitize();
    let td = dm.into_task_data()?;
    Ok(td)
}
