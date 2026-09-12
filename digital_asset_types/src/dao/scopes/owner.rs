use sea_orm::{entity::prelude::*, FromQueryResult};

use crate::dao::{
    asset,
    sea_orm_active_enums::{
        ChainMutability, OwnerType, RoyaltyTargetType, SpecificationAssetClass, SpecificationVersions,
    },
};

pub trait OwnerTokenInfo {
    fn get_owner(&self) -> &Vec<u8>;
    fn get_mint(&self) -> &Vec<u8>;
    fn get_balance(&self) -> Option<Decimal>;
    fn get_specification_asset_class(&self) -> &Option<SpecificationAssetClass>;
    fn get_price(&self) -> Option<f32>;
    fn get_decimals(&self) -> Option<i32>;
    fn get_supply(&self) -> Option<i64>;
    fn get_token_program(&self) -> &Option<Vec<u8>>;
    fn get_price_symbol(&self) -> &Option<String>;
    fn get_mint_authority(&self) -> &Option<Vec<u8>>;
    fn get_freeze_authority(&self) -> &Option<Vec<u8>>;
    fn get_token_accounts(&self) -> Option<Vec<(Vec<u8>, u64)>>;
    // Metadata fields (inlined from asset_data_v2 and offchain_metadata)
    fn get_metadata_url(&self) -> &Option<String>;
    fn get_chain_mutability(&self) -> &Option<ChainMutability>;
    fn get_chain_data(&self) -> &Option<serde_json::Value>;
    fn get_raw_name(&self) -> &Option<Vec<u8>>;
    fn get_raw_symbol(&self) -> &Option<Vec<u8>>;
    fn get_offchain_metadata(&self) -> &Option<serde_json::Value>;
}

impl OwnerTokenInfo for OwnerAssetInfo {
    fn get_mint(&self) -> &Vec<u8> {
        &self.mint
    }

    fn get_balance(&self) -> Option<Decimal> {
        self.balance
    }

    fn get_specification_asset_class(&self) -> &Option<SpecificationAssetClass> {
        &self.specification_asset_class
    }

    fn get_price(&self) -> Option<f32> {
        self.price
    }

    fn get_decimals(&self) -> Option<i32> {
        self.mint_decimals
    }

    fn get_supply(&self) -> Option<i64> {
        self.mint_supply
    }

    fn get_owner(&self) -> &Vec<u8> {
        &self.owner
    }

    fn get_token_program(&self) -> &Option<Vec<u8>> {
        &self.mint_token_program
    }

    fn get_price_symbol(&self) -> &Option<String> {
        &self.price_symbol
    }

    fn get_mint_authority(&self) -> &Option<Vec<u8>> {
        &self.mint_authority
    }

    fn get_freeze_authority(&self) -> &Option<Vec<u8>> {
        &self.mint_freeze_authority
    }

    fn get_token_accounts(&self) -> Option<Vec<(Vec<u8>, u64)>> {
        self.token_accounts.as_ref().and_then(|accounts| {
            let token_accounts: Vec<_> = accounts
                .as_array()?
                .iter()
                .filter_map(|account| {
                    let obj = account.as_object()?;
                    let balance = obj.get("balance")?.as_u64()?;
                    if balance == 0 {
                        return None;
                    }
                    let token_account = hex::decode(
                        obj.get("token_account")?
                            .as_str()?
                            .trim_start_matches("\\x"),
                    )
                    .ok()?;
                    Some((token_account, balance))
                })
                .collect();

            if token_accounts.is_empty() {
                None
            } else {
                Some(token_accounts)
            }
        })
    }

    fn get_metadata_url(&self) -> &Option<String> {
        &self.metadata_url
    }

    fn get_chain_mutability(&self) -> &Option<ChainMutability> {
        &self.chain_mutability
    }

    fn get_chain_data(&self) -> &Option<serde_json::Value> {
        &self.chain_data
    }

    fn get_raw_name(&self) -> &Option<Vec<u8>> {
        &self.raw_name
    }

    fn get_raw_symbol(&self) -> &Option<Vec<u8>> {
        &self.raw_symbol
    }

    fn get_offchain_metadata(&self) -> &Option<serde_json::Value> {
        &self.offchain_metadata
    }
}

impl OwnerTokenInfo for OwnerFungibleAssetInfo {
    fn get_mint(&self) -> &Vec<u8> {
        &self.mint
    }
    fn get_balance(&self) -> Option<Decimal> {
        self.balance
    }

    fn get_specification_asset_class(&self) -> &Option<SpecificationAssetClass> {
        &self.specification_asset_class
    }

    fn get_price(&self) -> Option<f32> {
        self.price
    }

    fn get_decimals(&self) -> Option<i32> {
        self.mint_decimals
    }

    fn get_supply(&self) -> Option<i64> {
        self.mint_supply
    }

    fn get_owner(&self) -> &Vec<u8> {
        &self.owner
    }

    fn get_token_program(&self) -> &Option<Vec<u8>> {
        &self.mint_token_program
    }

    fn get_price_symbol(&self) -> &Option<String> {
        &self.price_symbol
    }

    fn get_mint_authority(&self) -> &Option<Vec<u8>> {
        &self.mint_authority
    }
    fn get_freeze_authority(&self) -> &Option<Vec<u8>> {
        &self.mint_freeze_authority
    }

    fn get_token_accounts(&self) -> Option<Vec<(Vec<u8>, u64)>> {
        self.token_accounts.as_ref().and_then(|accounts| {
            let token_accounts: Vec<_> = accounts
                .as_array()?
                .iter()
                .filter_map(|account| {
                    let obj = account.as_object()?;
                    let balance = obj.get("balance")?.as_u64()?;
                    if balance == 0 {
                        return None;
                    }
                    let token_account = hex::decode(
                        obj.get("token_account")?
                            .as_str()?
                            .trim_start_matches("\\x"),
                    )
                    .ok()?;
                    Some((token_account, balance))
                })
                .collect();

            if token_accounts.is_empty() {
                None
            } else {
                Some(token_accounts)
            }
        })
    }

    fn get_metadata_url(&self) -> &Option<String> {
        &self.metadata_url
    }

    fn get_chain_mutability(&self) -> &Option<ChainMutability> {
        &self.chain_mutability
    }

    fn get_chain_data(&self) -> &Option<serde_json::Value> {
        &self.chain_data
    }

    fn get_raw_name(&self) -> &Option<Vec<u8>> {
        &self.raw_name
    }

    fn get_raw_symbol(&self) -> &Option<Vec<u8>> {
        &self.raw_symbol
    }

    fn get_offchain_metadata(&self) -> &Option<serde_json::Value> {
        &self.offchain_metadata
    }
}

#[derive(Clone, Debug, FromQueryResult)]
pub struct OwnerAssetInfo {
    pub owner: Vec<u8>,
    pub mint: Vec<u8>,
    pub balance: Option<Decimal>,
    pub metadata_account_id: Option<Vec<u8>>,
    pub token_accounts: Option<serde_json::Value>,

    // From tokens table
    pub mint_supply: Option<i64>,
    pub mint_decimals: Option<i32>,
    pub mint_authority: Option<Vec<u8>>,
    pub mint_freeze_authority: Option<Vec<u8>>,
    pub mint_close_authority: Option<Vec<u8>>,
    pub mint_extensions: Option<serde_json::Value>,
    pub mint_token_program: Option<Vec<u8>>,

    // From Asset table
    pub asset_frozen: Option<bool>,
    pub asset_delegate: Option<Vec<u8>>,
    pub specification_version: Option<SpecificationVersions>,
    pub specification_asset_class: Option<SpecificationAssetClass>,
    pub asset_owner_type: Option<OwnerType>,
    pub asset_supply: Option<i64>,
    pub asset_compressed: Option<bool>,
    pub asset_compressible: Option<bool>,
    pub asset_seq: Option<i64>,
    pub asset_tree_id: Option<Vec<u8>>,
    pub asset_leaf: Option<Vec<u8>>,
    pub asset_nonce: Option<i64>,
    pub asset_royalty_target_type: Option<RoyaltyTargetType>,
    pub asset_royalty_target: Option<Vec<u8>>,
    pub asset_royalty_amount: Option<i32>,
    pub asset_data: Option<Vec<u8>>,
    pub asset_burnt: Option<bool>,
    pub asset_data_hash: Option<String>,
    pub asset_creator_hash: Option<String>,
    pub asset_leaf_seq: Option<i64>,
    pub asset_creators_info: Option<serde_json::Value>,
    pub asset_collections_info: Option<serde_json::Value>,
    pub asset_authority_address: Option<Vec<u8>>,
    pub asset_authority_scopes: Option<Vec<String>>,
    pub asset_authority_slot_updated: Option<i64>,
    pub asset_authority_seq: Option<i64>,
    pub asset_authorities_info: Option<serde_json::Value>,
    pub asset_mint_extensions: Option<serde_json::Value>,
    pub mpl_core_plugins: Option<serde_json::Value>,
    pub mpl_core_plugins_json_version: Option<i32>,
    pub mpl_core_external_plugins: Option<serde_json::Value>,
    pub mpl_core_unknown_external_plugins: Option<serde_json::Value>,
    pub created_at: Option<DateTimeWithTimeZone>,
    pub edition_address: Option<Vec<u8>>,

    // From Price Table
    pub price: Option<f32>,
    pub price_symbol: Option<String>,

    // From asset_data_v2 table
    pub metadata_url: Option<String>,
    pub chain_mutability: Option<ChainMutability>,
    pub chain_data: Option<serde_json::Value>,
    pub raw_name: Option<Vec<u8>>,
    pub raw_symbol: Option<Vec<u8>>,

    // From offchain_metadata table
    pub offchain_metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, FromQueryResult)]
pub struct OwnerFungibleAssetInfo {
    pub owner: Vec<u8>,
    pub mint: Vec<u8>,
    pub balance: Option<Decimal>,
    pub token_accounts: Option<serde_json::Value>,

    // From asset table
    pub asset_data: Option<Vec<u8>>,
    pub asset_authority_address: Option<Vec<u8>>,
    pub asset_authority_scopes: Option<Vec<String>>,
    pub asset_authority_slot_updated: Option<i64>,
    pub asset_authority_seq: Option<i64>,
    pub asset_authorities_info: Option<serde_json::Value>,
    pub specification_asset_class: Option<SpecificationAssetClass>,
    pub asset_creators_info: Option<serde_json::Value>,
    pub asset_collections_info: Option<serde_json::Value>,

    // From tokens table
    pub mint_supply: Option<i64>,
    pub mint_decimals: Option<i32>,
    pub mint_authority: Option<Vec<u8>>,
    pub mint_freeze_authority: Option<Vec<u8>>,
    pub mint_close_authority: Option<Vec<u8>>,
    pub mint_extensions: Option<serde_json::Value>,
    pub mint_token_program: Option<Vec<u8>>,

    // From Price Table
    pub price: Option<f32>,
    pub price_symbol: Option<String>,

    // From asset_data_v2 table
    pub metadata_url: Option<String>,
    pub chain_mutability: Option<ChainMutability>,
    pub chain_data: Option<serde_json::Value>,
    pub raw_name: Option<Vec<u8>>,
    pub raw_symbol: Option<Vec<u8>>,

    // From offchain_metadata table
    pub offchain_metadata: Option<serde_json::Value>,
}

#[derive(FromQueryResult, Debug, Default, Clone, Eq, PartialEq)]
pub struct OwnerResult {
    pub owner: Vec<u8>,
    pub token_program: Option<Vec<u8>>,
}

impl From<OwnerAssetInfo> for asset::Model {
    fn from(owner_asset_info: OwnerAssetInfo) -> Self {
        asset::Model {
            id: owner_asset_info.mint.clone(),
            alt_id: None,
            specification_version: owner_asset_info
                .specification_version
                .or_else(|| Some(SpecificationVersions::V1)),
            specification_asset_class: owner_asset_info
                .specification_asset_class
                .or_else(|| Some(SpecificationAssetClass::FungibleToken)),
            owner: Some(owner_asset_info.owner.clone()),
            owner_type: owner_asset_info
                .asset_owner_type
                .unwrap_or(OwnerType::Token),
            delegate: owner_asset_info.asset_delegate,
            frozen: owner_asset_info.asset_frozen.unwrap_or_default(),
            supply: owner_asset_info
                .asset_supply
                .or_else(|| owner_asset_info.mint_supply)
                .unwrap_or_default(),
            mpl_core_plugins_json_version: owner_asset_info.mpl_core_plugins_json_version,
            mpl_core_plugins: owner_asset_info.mpl_core_plugins,
            mpl_core_external_plugins: owner_asset_info.mpl_core_external_plugins,
            mpl_core_unknown_external_plugins: owner_asset_info.mpl_core_unknown_external_plugins,
            supply_mint: Some(owner_asset_info.mint),
            compressed: owner_asset_info.asset_compressed.unwrap_or_default(),
            compressible: owner_asset_info.asset_compressible.unwrap_or_default(),
            seq: owner_asset_info.asset_seq,
            tree_id: owner_asset_info.asset_tree_id,
            leaf: owner_asset_info.asset_leaf,
            nonce: owner_asset_info.asset_nonce,
            royalty_target_type: owner_asset_info
                .asset_royalty_target_type
                .unwrap_or_default(),
            royalty_target: owner_asset_info.asset_royalty_target,
            royalty_amount: owner_asset_info.asset_royalty_amount.unwrap_or_default(),
            asset_data: owner_asset_info.asset_data,
            burnt: owner_asset_info.asset_burnt.unwrap_or_default(),
            leaf_seq: owner_asset_info.asset_leaf_seq,
            creators_info: owner_asset_info.asset_creators_info,
            collections_info: owner_asset_info.asset_collections_info,
            authority_address: owner_asset_info.asset_authority_address,
            authority_scopes: owner_asset_info.asset_authority_scopes,
            authority_seq: owner_asset_info.asset_authority_seq,
            authority_slot_updated: owner_asset_info.asset_authority_slot_updated,
            authorities_info: owner_asset_info.asset_authorities_info,
            mint_extensions: owner_asset_info
                .asset_mint_extensions
                .or_else(|| owner_asset_info.mint_extensions),

            created_at: owner_asset_info.created_at,
            data_hash: owner_asset_info.asset_data_hash,
            creator_hash: owner_asset_info.asset_creator_hash,
            metadata_account_id: owner_asset_info.metadata_account_id,
            edition_address: owner_asset_info.edition_address,
            // TODO: Remove this default. It's dangerous.
            ..Default::default()
        }
    }
}

impl From<OwnerFungibleAssetInfo> for asset::Model {
    fn from(owner_asset_info: OwnerFungibleAssetInfo) -> Self {
        asset::Model {
            id: owner_asset_info.mint.clone(),
            asset_data: owner_asset_info.asset_data,
            specification_version: Some(SpecificationVersions::V1),
            specification_asset_class: owner_asset_info
                .specification_asset_class
                .or_else(|| Some(SpecificationAssetClass::FungibleToken)),
            owner: Some(owner_asset_info.owner.clone()),
            owner_type: OwnerType::Token,
            supply: owner_asset_info.mint_supply.unwrap_or_default(),
            supply_mint: Some(owner_asset_info.mint),
            mint_extensions: owner_asset_info.mint_extensions,
            authority_address: owner_asset_info.asset_authority_address,
            authority_scopes: owner_asset_info.asset_authority_scopes,
            authority_seq: owner_asset_info.asset_authority_seq,
            authority_slot_updated: owner_asset_info.asset_authority_slot_updated,
            authorities_info: owner_asset_info.asset_authorities_info,
            creators_info: owner_asset_info.asset_creators_info,
            collections_info: owner_asset_info.asset_collections_info,
            ..Default::default()
        }
    }
}
