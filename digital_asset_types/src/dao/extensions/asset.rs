use sea_orm::{EntityTrait, EnumIter, Related, RelationDef, RelationTrait};

use crate::dao::{
    asset, asset_creators, asset_data_v2, editions, price,
    sea_orm_active_enums::{OwnerType, RoyaltyTargetType},
};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    AssetDataV2,
    AssetCreators,
    Price,
    MasterEdition,
    PrintEdition,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::AssetDataV2 => asset::Entity::belongs_to(asset_data_v2::Entity)
                .from(asset::Column::Id)
                .to(asset_data_v2::Column::Id)
                .into(),
            Self::AssetCreators => asset::Entity::has_many(asset_creators::Entity).into(),
            Self::Price => asset::Entity::belongs_to(price::Entity)
                .from(asset::Column::Id)
                .to(price::Column::Mint)
                .into(),

            Self::MasterEdition => asset::Entity::has_one(editions::Entity)
                .from(asset::Column::EditionAddress)
                .to(editions::Column::Id)
                .into(),
            Self::PrintEdition => asset::Entity::has_many(editions::Entity)
                .from(asset::Column::EditionAddress)
                .to(editions::Column::Parent)
                .into(),
        }
    }
}

impl Related<asset_data_v2::Entity> for asset::Entity {
    fn to() -> RelationDef {
        Relation::AssetDataV2.def()
    }
}

impl Related<asset_creators::Entity> for asset::Entity {
    fn to() -> RelationDef {
        Relation::AssetCreators.def()
    }
}

impl Related<price::Entity> for asset::Entity {
    fn to() -> RelationDef {
        Relation::Price.def()
    }
}

impl Related<editions::Entity> for asset::Entity {
    fn to() -> RelationDef {
        Relation::MasterEdition.def()
    }
}

impl Default for RoyaltyTargetType {
    fn default() -> Self {
        Self::Creators
    }
}

impl Default for asset::Model {
    fn default() -> Self {
        Self {
            id: vec![],
            alt_id: None,
            specification_version: None,
            specification_asset_class: None,
            owner: None,
            owner_type: OwnerType::Single,
            delegate: None,
            frozen: Default::default(),
            supply: Default::default(),
            supply_mint: None,
            compressed: Default::default(),
            compressible: Default::default(),
            seq: None,
            tree_id: None,
            leaf: None,
            nonce: None,
            royalty_target_type: RoyaltyTargetType::Unknown,
            royalty_target: None,
            royalty_amount: Default::default(),
            asset_data: None,
            created_at: None,
            burnt: Default::default(),
            slot_updated: None,
            slot_updated_metadata_account: None,
            slot_updated_mint_account: None,
            slot_updated_token_account: None,
            slot_updated_cnft_transaction: None,
            data_hash: None,
            creator_hash: None,
            owner_delegate_seq: None,
            leaf_seq: None,
            creators_info: None,
            collections_info: None,
            authorities_info: None,
            mint_extensions: None,
            token_extensions: None,
            metadata_account_id: None,
            base_info_seq: None,
            edition_address: None,
            mpl_core_plugins: None,
            mpl_core_unknown_plugins: None,
            mpl_core_collection_current_size: None,
            mpl_core_collection_num_minted: None,
            mpl_core_plugins_json_version: None,
            mpl_core_external_plugins: None,
            mpl_core_unknown_external_plugins: None,
            authority_address: None,
            authority_seq: None,
            authority_slot_updated: None,
            authority_scopes: None,
            collection_hash: None,
            asset_data_hash: None,
            bubblegum_flags: None,
            non_transferable: None,
            t22_metadata_address: None,
            is_agent: false,
            agent_token: None,
            asset_signer: None,
            slot_updated_agent_registry: None,
        }
    }
}
