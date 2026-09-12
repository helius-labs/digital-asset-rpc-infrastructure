use schemars::JsonSchema;

use super::{Owner, TokenAccount};
use {
    crate::rpc::Asset,
    serde::{Deserialize, Serialize},
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
pub struct AssetError {
    pub id: String,
    pub error: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
pub struct GetGroupingResponse {
    pub group_key: String,
    pub group_name: String,
    pub group_size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
#[allow(non_snake_case)]
pub struct AssetList {
    pub last_indexed_slot: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grand_total: Option<u64>,
    pub total: u32,
    pub limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub items: Vec<Asset>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<AssetError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nativeBalance: Option<NativeBalance>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
#[allow(non_snake_case)]
pub struct GetAssetsV2Response {
    pub last_indexed_slot: u64,
    pub items: Vec<Option<Asset>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
pub struct NativeBalance {
    pub lamports: u64,
    pub price_per_sol: f64,
    pub total_price: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
pub struct TransactionSignatureList {
    pub last_indexed_slot: u64,
    pub total: u32,
    pub limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    pub items: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
pub struct OwnerList {
    pub last_indexed_slot: u64,
    pub total: u32,
    pub limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    pub owners: Vec<Owner>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
pub struct TokenAccountsList {
    pub last_indexed_slot: u64,
    pub total: u32,
    pub limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    pub token_accounts: Vec<TokenAccount>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
pub struct Edition {
    pub mint: String,
    pub edition_address: String,
    pub edition: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, JsonSchema)]
#[serde(default)]
pub struct EditionsList {
    pub last_indexed_slot: u64,
    pub total: u32,
    pub limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    pub master_edition_address: String,
    pub supply: u64,
    pub max_supply: Option<u64>,
    pub editions: Vec<Edition>,
}
