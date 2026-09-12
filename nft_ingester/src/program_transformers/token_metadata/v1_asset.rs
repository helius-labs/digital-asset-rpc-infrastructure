use crate::config::IngesterConfig;
use crate::program_transformers::asset_upserts::{
    upsert_assets_metadata_account_columns, upsert_assets_mint_account_columns,
    upsert_assets_token_account_columns, AssetMetadataAccountColumns, AssetMintAccountColumns,
    AssetTokenAccountColumns,
};
use crate::program_transformers::utils::find_model_with_retry;
use crate::tasks::{DownloadMetadata, IntoTaskData};
use crate::{error::IngesterError, tasks::TaskData};
use chrono::Utc;
use crate::program_transformers::bubblegum::upsert_creators_info_in_asset_raw;
use digital_asset_types::dao::{
    asset_data_v2, offchain_metadata, owners, AuthorityInfo, CollectionsInfo, CreatorInfo,
};
use digital_asset_types::{
    dao::{
        asset, asset_creators,
        sea_orm_active_enums::{
            ChainMutability, Mutability, OwnerType, SpecificationAssetClass, SpecificationVersions,
        },
        tokens,
    },
    json::ChainDataV1,
};
use lazy_static::lazy_static;
use log::warn;
use mpl_token_metadata::accounts::{MasterEdition, Metadata};
use mpl_token_metadata::types::{CollectionDetails, TokenStandard};
use plerkle_serialization::Pubkey as FBPubkey;
use sea_orm::{
    entity::*, query::*, sea_query::OnConflict, ActiveValue::Set, ConnectionTrait, DbBackend,
    DbErr, EntityTrait, JsonValue,
};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

pub async fn burn_v1_asset<T: ConnectionTrait + TransactionTrait>(
    conn: &T,
    id: FBPubkey,
    slot: u64,
) -> Result<(), IngesterError> {
    let (id, slot_i) = (id.0, slot as i64);
    let model = asset::ActiveModel {
        id: Set(id.to_vec()),
        slot_updated_metadata_account: Set(Some(slot_i)),
        burnt: Set(true),
        ..Default::default()
    };
    let mut query = asset::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns([
                    asset::Column::SlotUpdatedMetadataAccount,
                    asset::Column::Burnt,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    query.sql = format!(
        "{} WHERE excluded.slot_updated_metadata_account > asset.slot_updated_metadata_account OR asset.slot_updated_metadata_account IS NULL",
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
                supply: token.supply as u64,
                supply_mint: Some(token.mint.clone()),
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
    let token_account: Option<owners::Model> = find_model_with_retry(
        conn,
        "owners",
        &owners::Entity::find()
            .filter(owners::Column::Mint.eq(mint_pubkey_vec.clone()))
            .filter(owners::Column::TokenAmount.gt(0))
            .order_by(owners::Column::SlotUpdated, Order::Desc),
        RETRY_INTERVALS,
    )
    .await
    .map_err(|e: DbErr| IngesterError::DatabaseError(e.to_string()))?;

    if let Some(token_account) = token_account {
        upsert_assets_token_account_columns(
            AssetTokenAccountColumns {
                mint: mint_pubkey_vec.clone(),
                owner: token_account.owner,
                delegate: token_account.delegate,
                frozen: token_account.frozen,
                token_extensions: token_account.token_extensions,
                slot_updated_token_account: token_account.slot_updated,
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
    _config: &IngesterConfig,
    conn: &T,
    metadata: &Metadata,
    metadata_account_id: FBPubkey,
    slot: u64,
) -> Result<Option<TaskData>, IngesterError> {
    let metadata = metadata.clone();
    //let data = metadata.data;
    let mint_pubkey = metadata.mint;
    let mint_pubkey_array = mint_pubkey.to_bytes();
    let mint_pubkey_vec = mint_pubkey_array.to_vec();

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
        if supply == 1 {
            ownership_type = OwnerType::Single;
        } else if supply > 1 {
            ownership_type = OwnerType::Token;
        }
    }

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
        .map_err(|e| IngesterError::SerializatonError(e.to_string()))?;
    let chain_mutability = match metadata.is_mutable {
        true => ChainMutability::Mutable,
        false => ChainMutability::Immutable,
    };

    let offchain_metadata_model = offchain_metadata::ActiveModel {
        metadata_url: Set(uri.clone()),
        metadata: Set(JsonValue::String("processing".to_string())),
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
    let asset_data_v2_model = asset_data_v2::ActiveModel {
        metadata_url: Set(uri.clone()),
        id: Set(mint_pubkey_array.to_vec()),
        chain_mutability: Set(chain_mutability),
        chain_data: Set(chain_data_json),
        slot_updated: Set(slot_i),
        base_info_seq: Set(Some(0)),
        raw_name: Set(Some(name.to_vec())),
        raw_symbol: Set(Some(symbol.to_vec())),
    };
    let mut asset_data_v2_query = asset_data_v2::Entity::insert(asset_data_v2_model)
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
    asset_data_v2_query.sql = format!(
        "{} WHERE excluded.slot_updated >= asset_data_v2.slot_updated",
        asset_data_v2_query.sql
    );

    let txn = conn.begin().await?;
    txn.execute(offchain_metadata_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    txn.execute(asset_data_v2_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    upsert_assets_metadata_account_columns(
        AssetMetadataAccountColumns {
            mint: mint_pubkey_vec.clone(),
            metadata_account_id: metadata_account_id.0.to_vec(),
            owner_type: ownership_type,
            specification_asset_class: Some(class),
            slot_updated_metadata_account: slot_i as u64,
            asset_data: Some(mint_pubkey_vec.clone()),
            royalty_amount: metadata.seller_fee_basis_points as i32,
            mpl_core_external_plugins: None,
            mpl_core_unknown_external_plugins: None,
            mpl_core_collection_num_minted: None,
            mpl_core_collection_current_size: None,
            mpl_core_plugins_json_version: None,
            mpl_core_plugins: None,
            mpl_core_unknown_plugins: None,
            is_agent: false,
            asset_signer: None,
        },
        &txn,
    )
    .await
    .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    let metadata_creators = metadata.creators.unwrap_or_default();

    let creators = metadata_creators
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

        // Upsert creators_info JSONB column for optimized queries.
        let creator_infos: Vec<CreatorInfo> = metadata_creators
            .iter()
            .map(|c| CreatorInfo {
                creator: c.address.to_bytes().to_vec(),
                share: Some(c.share),
                verified: c.verified,
            })
            .collect();
        upsert_creators_info_in_asset_raw(&txn, mint_pubkey_vec.clone(), creator_infos, slot_i, 0)
            .await?;
    }

    let edition_address = MasterEdition::find_pda(&mint_pubkey).0;
    let asset_model = asset::ActiveModel {
        id: Set(mint_pubkey_vec.clone()),
        edition_address: Set(Some(edition_address.to_bytes().to_vec())),
        ..Default::default()
    };
    let mut query = asset::Entity::insert(asset_model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns([asset::Column::EditionAddress])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    query.sql = format!(
            "{} WHERE asset.edition_address IS NULL OR excluded.slot_updated_metadata_account >= asset.slot_updated_metadata_account OR asset.slot_updated_metadata_account IS NULL",
            query.sql);

    txn.execute(query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    let authority_info = AuthorityInfo {
        authority: authority.clone(),
        seq: 0,
        slot_updated: slot_i,
        scopes: None,
    };

    let mut asset_model = asset::ActiveModel {
        id: Set(mint_pubkey_vec.clone()),
        authorities_info: Set(Some(authority_info.into())),
        authority_address: Set(Some(authority)),
        authority_seq: Set(Some(0)),
        authority_slot_updated: Set(Some(slot_i)),
        authority_scopes: Set(None),
        ..Default::default()
    };
    let mut collections_info = match &metadata.collection {
        Some(c) => CollectionsInfo {
            collection_id: Some(c.key.to_string()),
            seq: None,
            slot_updated: slot_i,
            verified: c.verified,
            collection_info_seq: None,
            ..Default::default()
        },
        None => CollectionsInfo {
            collection_id: None,
            seq: None,
            slot_updated: slot_i,
            verified: false,
            collection_info_seq: None,
            ..Default::default()
        },
    };

    if let Some(collection_details) = &metadata.collection_details {
        match collection_details {
            CollectionDetails::V1 { size } => {
                collections_info.collection_size = Some(*size);
                collections_info.collection_nft = Some(true);
            }
            CollectionDetails::V2 { .. } => {
                // V2 collection details - padding only, no size field
                collections_info.collection_nft = Some(true);
            }
        }
    }
    asset_model.collections_info = Set(Some(collections_info.into()));

    let mut asset_query = asset::Entity::insert(asset_model)
        .on_conflict(OnConflict::columns([asset::Column::Id]))
        .build(DbBackend::Postgres);

    asset_query.sql = format!(
            "{} DO UPDATE SET
            authorities_info = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN COALESCE(excluded.authorities_info, '{{}}') ELSE asset.authorities_info END,
            authority_address = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN excluded.authority_address ELSE asset.authority_address END,
            authority_slot_updated = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN excluded.authority_slot_updated ELSE asset.authority_slot_updated END,
            authority_scopes = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN excluded.authority_scopes ELSE asset.authority_scopes END,
            collections_info = CASE WHEN COALESCE((excluded.collections_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.collections_info->>'slot_updated')::bigint, -1) THEN COALESCE(excluded.collections_info, '{{}}') ELSE asset.collections_info END
            WHERE asset.id = excluded.id",
            asset_query.sql
        );

    txn.execute(asset_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

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
