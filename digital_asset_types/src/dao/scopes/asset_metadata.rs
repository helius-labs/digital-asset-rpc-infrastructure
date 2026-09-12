use crate::{
    dao::{
        asset, asset_data_v2, offchain_metadata,
        sea_orm_active_enums::{
            ChainMutability, OwnerType, RoyaltyTargetType, SpecificationAssetClass,
            SpecificationVersions,
        },
        AssetMetadata,
    },
    metric,
};
use cadence_macros::statsd_count;
use log::error;
use sea_orm::{
    prelude::DateTimeWithTimeZone, Condition, ConnectionTrait, DbErr, EntityTrait, FromQueryResult,
    QueryFilter, QuerySelect,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, FromQueryResult)]
pub struct AssetWithMetadata {
    pub id: Vec<u8>,
    pub alt_id: Option<Vec<u8>>,
    pub metadata_account_id: Option<Vec<u8>>,
    pub specification_version: Option<SpecificationVersions>,
    pub specification_asset_class: Option<SpecificationAssetClass>,
    pub owner: Option<Vec<u8>>,
    pub owner_type: OwnerType,
    pub delegate: Option<Vec<u8>>,
    pub frozen: bool,
    pub supply: i64,
    pub supply_mint: Option<Vec<u8>>,
    pub compressed: bool,
    pub compressible: bool,
    pub seq: Option<i64>,
    pub tree_id: Option<Vec<u8>>,
    pub leaf: Option<Vec<u8>>,
    pub nonce: Option<i64>,
    pub royalty_target_type: RoyaltyTargetType,
    pub royalty_target: Option<Vec<u8>>,
    pub royalty_amount: i32,
    pub asset_data: Option<Vec<u8>>,
    pub created_at: Option<DateTimeWithTimeZone>,
    pub burnt: bool,
    pub slot_updated: Option<i64>, // Legacy field. Remove after migration.
    pub slot_updated_metadata_account: Option<i64>,
    pub slot_updated_mint_account: Option<i64>,
    pub slot_updated_token_account: Option<i64>,
    pub slot_updated_cnft_transaction: Option<i64>,
    pub data_hash: Option<String>,
    pub creator_hash: Option<String>,
    pub owner_delegate_seq: Option<i64>,
    pub leaf_seq: Option<i64>,
    pub creators_info: Option<serde_json::Value>,
    pub collections_info: Option<serde_json::Value>,
    pub authority_address: Option<Vec<u8>>,
    pub authority_scopes: Option<Vec<String>>,
    pub authority_slot_updated: Option<i64>,
    pub authority_seq: Option<i64>,
    pub authorities_info: Option<serde_json::Value>,
    pub mint_extensions: Option<serde_json::Value>,
    pub token_extensions: Option<serde_json::Value>,
    pub metadata_url: String,
    pub chain_data: serde_json::Value,
    pub chain_mutability: ChainMutability,
    pub raw_name: Option<Vec<u8>>,
    pub raw_symbol: Option<Vec<u8>>,
    pub metadata: serde_json::Value,
    pub base_info_seq: Option<i64>,
    pub edition_address: Option<Vec<u8>>,
    pub mpl_core_plugins: Option<serde_json::Value>,
    pub mpl_core_unknown_plugins: Option<serde_json::Value>,
    pub mpl_core_collection_num_minted: Option<i32>,
    pub mpl_core_collection_current_size: Option<i32>,
    pub mpl_core_plugins_json_version: Option<i32>,
    pub mpl_core_external_plugins: Option<serde_json::Value>,
    pub mpl_core_unknown_external_plugins: Option<serde_json::Value>,
    pub collection_hash: Option<String>,
    pub asset_data_hash: Option<String>,
    pub bubblegum_flags: Option<i16>,
    pub non_transferable: Option<bool>,
    pub t22_metadata_address: Option<Vec<u8>>,
    pub is_agent: bool,
    pub agent_token: Option<Vec<u8>>,
    pub asset_signer: Option<Vec<u8>>,
    pub slot_updated_agent_registry: Option<i64>,
}

pub async fn get_asset_and_metadata(
    conn: &impl ConnectionTrait,
    cond: Condition,
    verify_asset_exists: bool,
) -> Result<(asset::Model, AssetMetadata), DbErr> {
    let asset_with_metadata = asset::Entity::find()
        .join(
            sea_orm::JoinType::InnerJoin,
            asset::Entity::belongs_to(asset_data_v2::Entity)
                .from(asset::Column::Id)
                .to(asset_data_v2::Column::Id)
                .into(),
        )
        .join(
            sea_orm::JoinType::InnerJoin,
            asset_data_v2::Entity::belongs_to(offchain_metadata::Entity)
                .from(asset_data_v2::Column::MetadataUrl)
                .to(offchain_metadata::Column::MetadataUrl)
                .into(),
        )
        .select_only()
        .column(asset::Column::Id)
        .column(asset::Column::AltId)
        .column(asset::Column::MetadataAccountId)
        .column(asset::Column::SpecificationVersion)
        .column(asset::Column::SpecificationAssetClass)
        .column(asset::Column::Owner)
        .column(asset::Column::OwnerType)
        .column(asset::Column::Delegate)
        .column(asset::Column::Frozen)
        .column(asset::Column::Supply)
        .column(asset::Column::SupplyMint)
        .column(asset::Column::Compressed)
        .column(asset::Column::Compressible)
        .column(asset::Column::Seq)
        .column(asset::Column::TreeId)
        .column(asset::Column::Leaf)
        .column(asset::Column::Nonce)
        .column(asset::Column::RoyaltyTargetType)
        .column(asset::Column::RoyaltyTarget)
        .column(asset::Column::RoyaltyAmount)
        .column(asset::Column::AssetData)
        .column(asset::Column::CreatedAt)
        .column(asset::Column::Burnt)
        .column(asset::Column::SlotUpdated)
        .column(asset::Column::SlotUpdatedMetadataAccount)
        .column(asset::Column::SlotUpdatedMintAccount)
        .column(asset::Column::SlotUpdatedTokenAccount)
        .column(asset::Column::SlotUpdatedCnftTransaction)
        .column(asset::Column::DataHash)
        .column(asset::Column::CreatorHash)
        .column(asset::Column::OwnerDelegateSeq)
        .column(asset::Column::LeafSeq)
        .column(asset::Column::CreatorsInfo)
        .column(asset::Column::CollectionsInfo)
        .column(asset::Column::AuthoritiesInfo)
        .column(asset::Column::MintExtensions)
        .column(asset::Column::TokenExtensions)
        .column(asset::Column::BaseInfoSeq)
        .column(asset::Column::EditionAddress)
        .column(asset::Column::MplCorePlugins)
        .column(asset::Column::MplCoreUnknownPlugins)
        .column(asset::Column::MplCoreCollectionNumMinted)
        .column(asset::Column::MplCoreCollectionCurrentSize)
        .column(asset::Column::MplCorePluginsJsonVersion)
        .column(asset::Column::MplCoreExternalPlugins)
        .column(asset::Column::MplCoreUnknownExternalPlugins)
        .column(asset::Column::AuthorityAddress)
        .column(asset::Column::AuthoritySeq)
        .column(asset::Column::AuthorityScopes)
        .column(asset::Column::AuthoritySlotUpdated)
        .column(asset::Column::CollectionHash)
        .column(asset::Column::AssetDataHash)
        .column(asset::Column::BubblegumFlags)
        .column(asset::Column::NonTransferable)
        .column(asset::Column::T22MetadataAddress)
        .column(asset::Column::IsAgent)
        .column(asset::Column::AgentToken)
        .column(asset::Column::AssetSigner)
        .column(asset::Column::SlotUpdatedAgentRegistry)
        .column(asset_data_v2::Column::MetadataUrl)
        .column(asset_data_v2::Column::ChainData)
        .column(asset_data_v2::Column::ChainMutability)
        .column(asset_data_v2::Column::RawName)
        .column(asset_data_v2::Column::RawSymbol)
        .column(offchain_metadata::Column::Metadata)
        .filter(cond.clone())
        .into_model::<AssetWithMetadata>()
        .one(conn)
        .await?;

    match asset_with_metadata {
        None => {
            if verify_asset_exists {
                // In case data exists in `asset` table bu not `asset_data_v2`, the above query will return None.
                // In this case, we'll check if the data exists in `asset` table.
                // Ideally this should never happen.
                // But as a safety measure, we're enabling this flag to ensure that the roll-out is smooth.
                // Note that this extra call adds additional latency. So we'll disable this flag once we're confident.
                let asset = asset::Entity::find().filter(cond).one(conn).await?;
                if let Some(asset) = asset {
                    if asset.specification_asset_class
                        != Some(SpecificationAssetClass::FungibleAsset)
                    {
                        // asset is present in `asset` table but not in `asset_data_v2` table.
                        error!(
                            target: "asset_data_v2_missing",
                            "Asset data not found in `asset_data_v2` table. Asset ID: {}",
                            bs58::encode(asset.id).into_string()
                        );
                        metric! {
                            statsd_count!("das_api.asset_data_v2_missing", 1);
                        }
                    }
                }
            }
            Err(DbErr::RecordNotFound("Asset Not Found".to_string()))
        }
        Some(awm) => {
            let asset = asset::Model {
                id: awm.id.clone(),
                alt_id: awm.alt_id,
                metadata_account_id: awm.metadata_account_id,
                specification_version: awm.specification_version,
                specification_asset_class: awm.specification_asset_class,
                owner: awm.owner,
                owner_type: awm.owner_type,
                delegate: awm.delegate,
                frozen: awm.frozen,
                supply: awm.supply,
                supply_mint: awm.supply_mint,
                compressed: awm.compressed,
                compressible: awm.compressible,
                seq: awm.seq,
                tree_id: awm.tree_id,
                leaf: awm.leaf,
                nonce: awm.nonce,
                royalty_target_type: awm.royalty_target_type,
                royalty_target: awm.royalty_target,
                royalty_amount: awm.royalty_amount,
                asset_data: awm.asset_data,
                created_at: awm.created_at,
                burnt: awm.burnt,
                slot_updated: awm.slot_updated,
                slot_updated_metadata_account: awm.slot_updated_metadata_account,
                slot_updated_mint_account: awm.slot_updated_mint_account,
                slot_updated_token_account: awm.slot_updated_token_account,
                slot_updated_cnft_transaction: awm.slot_updated_cnft_transaction,
                data_hash: awm.data_hash,
                creator_hash: awm.creator_hash,
                owner_delegate_seq: awm.owner_delegate_seq,
                leaf_seq: awm.leaf_seq,
                creators_info: awm.creators_info,
                collections_info: awm.collections_info,
                authority_address: awm.authority_address,
                authority_scopes: awm.authority_scopes,
                authority_seq: awm.authority_seq,
                authority_slot_updated: awm.authority_slot_updated,
                authorities_info: awm.authorities_info,
                mint_extensions: awm.mint_extensions,
                token_extensions: awm.token_extensions,
                base_info_seq: awm.base_info_seq,
                edition_address: awm.edition_address,
                mpl_core_plugins: awm.mpl_core_plugins,
                mpl_core_unknown_plugins: awm.mpl_core_unknown_plugins,
                mpl_core_collection_num_minted: awm.mpl_core_collection_num_minted,
                mpl_core_collection_current_size: awm.mpl_core_collection_current_size,
                mpl_core_plugins_json_version: awm.mpl_core_plugins_json_version,
                mpl_core_external_plugins: awm.mpl_core_external_plugins,
                mpl_core_unknown_external_plugins: awm.mpl_core_unknown_external_plugins,
                collection_hash: awm.collection_hash,
                asset_data_hash: awm.asset_data_hash,
                bubblegum_flags: awm.bubblegum_flags,
                non_transferable: awm.non_transferable,
                t22_metadata_address: awm.t22_metadata_address,
                is_agent: awm.is_agent,
                agent_token: awm.agent_token,
                asset_signer: awm.asset_signer,
                slot_updated_agent_registry: awm.slot_updated_agent_registry,
            };
            let asset_metadata = AssetMetadata {
                id: awm.id,
                metadata_url: awm.metadata_url,
                chain_data: awm.chain_data,
                chain_mutability: awm.chain_mutability,
                raw_name: awm.raw_name,
                raw_symbol: awm.raw_symbol,
                metadata: awm.metadata,
            };

            Ok((asset, asset_metadata))
        }
    }
}
