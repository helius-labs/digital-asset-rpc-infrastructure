use super::sea_orm_active_enums::ChainMutability;
use crate::{
    dao::{asset, asset_creators},
    rpc::{Authority, Group, GroupDefinition},
};
use schemars::JsonSchema;
use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FullAsset {
    pub asset: asset::Model,
    pub data: AssetMetadata,
    pub authorities: Vec<Authority>,
    pub creators: Vec<asset_creators::Model>,
    pub groups: Vec<Group>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_info: Option<TokenInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editions: Option<EditionInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_definition: Option<GroupDefinition>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct AssetRelated {
    pub authorities: Vec<Authority>,
    pub creators: Vec<asset_creators::Model>,
    pub groups: Vec<Group>,
}

pub struct FullAssetList {
    pub list: Vec<FullAsset>,
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Serialize, Deserialize, Default)]
pub struct EditionInfo {
    pub address: String,
    pub edition_type: String,
    pub parent: Option<String>,
    pub master_edition_mint: Option<String>,
    pub edition: Option<u64>,
    pub supply: Option<u64>,
    pub max_supply: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Serialize, Deserialize)]
pub struct TokenAccount {
    pub address: String,
    pub balance: u64,
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Serialize, Deserialize, Default)]
pub struct TokenInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_accounts: Option<Vec<TokenAccount>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supply: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_program: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associated_token_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_info: Option<PriceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mint_authority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freeze_authority: Option<String>,
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Serialize, Deserialize)]
pub struct PriceInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_per_token: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, FromQueryResult)]
pub struct AssetMetadata {
    pub id: Vec<u8>,
    pub metadata_url: String,
    pub chain_data: serde_json::Value,
    pub chain_mutability: ChainMutability,
    pub raw_name: Option<Vec<u8>>,
    pub raw_symbol: Option<Vec<u8>>,
    pub metadata: serde_json::Value,
}

impl Default for AssetMetadata {
    fn default() -> Self {
        Self {
            id: vec![],
            metadata_url: "".to_string(),
            chain_data: serde_json::Value::Null,
            chain_mutability: ChainMutability::Mutable,
            raw_name: None,
            raw_symbol: None,
            metadata: serde_json::Value::Null,
        }
    }
}
