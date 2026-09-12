use crate::dao::{
    sea_orm_active_enums::{
        OwnerType, RoyaltyTargetType, SpecificationAssetClass, SpecificationVersions,
    },
    TokenInfo,
};
use serde_json::Value;
#[cfg(feature = "sql_types")]
use std::collections::BTreeMap;

use crate::dao::sea_orm_active_enums::ChainMutability;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use {
    serde::{Deserialize, Serialize},
    std::collections::HashMap,
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AssetProof {
    pub last_indexed_slot: u64,
    pub root: String,
    pub proof: Vec<String>,
    pub node_index: i64,
    pub leaf: String,
    pub tree_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
pub enum Interface {
    #[serde(rename = "V1_NFT")]
    V1NFT,
    #[serde(rename = "V1_PRINT")]
    V1PRINT,
    #[allow(non_camel_case_types)]
    #[serde(rename = "LEGACY_NFT")]
    LEGACY_NFT,
    #[serde(rename = "V2_NFT")]
    Nft,
    #[serde(rename = "FungibleAsset")]
    FungibleAsset,
    #[serde(rename = "FungibleToken")]
    FungibleToken,
    #[serde(rename = "Custom")]
    Custom,
    #[serde(rename = "Identity")]
    Identity,
    #[serde(rename = "Executable")]
    Executable,
    #[serde(rename = "ProgrammableNFT")]
    ProgrammableNFT,
    #[serde(rename = "MplCoreAsset")]
    MplCoreAsset,
    #[serde(rename = "MplBubblegumV2")]
    MplBubblegumV2,
    #[serde(rename = "MplCoreCollection")]
    MplCoreCollection,
    #[serde(rename = "MplCoreGroup")]
    MplCoreGroup,
}

impl From<(&SpecificationVersions, &SpecificationAssetClass, &u64)> for Interface {
    fn from(i: (&SpecificationVersions, &SpecificationAssetClass, &u64)) -> Self {
        match i {
            // Temporary hack to fix marking of t22s as NFTs.
            (SpecificationVersions::V1, SpecificationAssetClass::Nft, supply) if *supply > 1 => {
                Interface::FungibleToken
            }
            (SpecificationVersions::V1, SpecificationAssetClass::Nft, _)
            | (SpecificationVersions::V1, SpecificationAssetClass::PrintableNft, _) => {
                Interface::V1NFT
            }

            (SpecificationVersions::V0, SpecificationAssetClass::Nft, _) => Interface::LEGACY_NFT,

            (SpecificationVersions::V1, SpecificationAssetClass::FungibleAsset, _) => {
                Interface::FungibleAsset
            }
            (SpecificationVersions::V1, SpecificationAssetClass::FungibleToken, _) => {
                Interface::FungibleToken
            }

            (SpecificationVersions::V1, SpecificationAssetClass::ProgrammableNft, _) => {
                Interface::ProgrammableNFT
            }

            (SpecificationVersions::V1, SpecificationAssetClass::Unknown, supply)
                if *supply > 1 =>
            {
                Interface::FungibleToken
            }
            (_, SpecificationAssetClass::MplCoreAsset, _) => Interface::MplCoreAsset,
            (_, SpecificationAssetClass::MplCoreCollection, _) => Interface::MplCoreCollection,
            (_, SpecificationAssetClass::MplCoreGroup, _) => Interface::MplCoreGroup,
            (_, SpecificationAssetClass::MplBubblegumV2, _) => Interface::MplBubblegumV2,
            _ => Interface::Custom,
        }
    }
}

impl Into<(SpecificationVersions, SpecificationAssetClass)> for Interface {
    fn into(self) -> (SpecificationVersions, SpecificationAssetClass) {
        match self {
            Interface::V1NFT => (SpecificationVersions::V1, SpecificationAssetClass::Nft),
            Interface::LEGACY_NFT => (SpecificationVersions::V0, SpecificationAssetClass::Nft),
            Interface::ProgrammableNFT => (
                SpecificationVersions::V1,
                SpecificationAssetClass::ProgrammableNft,
            ),
            Interface::V1PRINT => (SpecificationVersions::V1, SpecificationAssetClass::Print),
            Interface::FungibleAsset => (
                SpecificationVersions::V1,
                SpecificationAssetClass::FungibleAsset,
            ),
            Interface::FungibleToken => (
                SpecificationVersions::V1,
                SpecificationAssetClass::FungibleToken,
            ),
            Interface::MplCoreAsset => (
                SpecificationVersions::V1,
                SpecificationAssetClass::MplCoreAsset,
            ),
            Interface::MplCoreCollection => (
                SpecificationVersions::V1,
                SpecificationAssetClass::MplCoreCollection,
            ),
            Interface::MplCoreGroup => (
                SpecificationVersions::V1,
                SpecificationAssetClass::MplCoreGroup,
            ),
            _ => (SpecificationVersions::V1, SpecificationAssetClass::Unknown),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Quality {
    #[serde(rename = "$$schema")]
    pub schema: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
pub enum Context {
    #[serde(rename = "wallet-default")]
    WalletDefault,
    #[serde(rename = "web-desktop")]
    WebDesktop,
    #[serde(rename = "web-mobile")]
    WebMobile,
    #[serde(rename = "app-mobile")]
    AppMobile,
    #[serde(rename = "app-desktop")]
    AppDesktop,
    #[serde(rename = "app")]
    App,
    #[serde(rename = "vr")]
    Vr,
}

pub type Contexts = Vec<Context>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct File {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdn_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<Quality>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<Contexts>,
}

pub type Files = Vec<File>;

#[derive(PartialEq, Eq, Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MetadataMap(BTreeMap<String, serde_json::Value>);

impl MetadataMap {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn inner(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.0
    }

    pub fn set_item(&mut self, key: &str, value: serde_json::Value) -> &mut Self {
        self.0.insert(key.to_string(), value);
        self
    }

    pub fn get_item(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.get(key)
    }
}

// TODO sub schema support
pub type Links = HashMap<String, serde_json::Value>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Content {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub json_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Files>,
    pub metadata: MetadataMap,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Links>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum Scope {
    #[serde(rename = "full")]
    Full,
    #[serde(rename = "royalty")]
    Royalty,
    #[serde(rename = "metadata")]
    Metadata,
    #[serde(rename = "extension")]
    Extension,
}

impl From<String> for Scope {
    fn from(s: String) -> Self {
        match &*s {
            "royalty" => Scope::Royalty,
            "metadata" => Scope::Metadata,
            "extension" => Scope::Extension,
            _ => Scope::Full,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Authority {
    pub address: String,
    pub scopes: Vec<Scope>,

    // Used internally
    #[serde(skip_serializing)]
    pub asset_id: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Compression {
    pub eligible: bool,
    pub compressed: bool,
    pub data_hash: String,
    pub creator_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_data_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u8>,
    pub asset_hash: String,
    pub tree: String,
    pub seq: i64,
    pub leaf_id: i64,
}

pub type GroupKey = String;
pub type GroupValue = String;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Group {
    pub group_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_metadata: Option<CollectionMetadata>,

    // Used internally
    #[serde(skip_serializing)]
    pub asset_id: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct GroupDefinition {
    pub group_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    // Used internally
    #[serde(skip_serializing)]
    pub asset_id: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CollectionMetadata {
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub image: Option<String>,
    pub description: Option<String>,
    pub external_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
pub enum RoyaltyModel {
    #[serde(rename = "creators")]
    Creators,
    #[serde(rename = "fanout")]
    Fanout,
    #[serde(rename = "single")]
    Single,
}

impl From<String> for RoyaltyModel {
    fn from(s: String) -> Self {
        match &*s {
            "creators" => RoyaltyModel::Creators,
            "fanout" => RoyaltyModel::Fanout,
            "single" => RoyaltyModel::Single,
            _ => RoyaltyModel::Creators,
        }
    }
}

#[cfg(feature = "sql_types")]
impl From<RoyaltyTargetType> for RoyaltyModel {
    fn from(s: RoyaltyTargetType) -> Self {
        match s {
            RoyaltyTargetType::Creators => RoyaltyModel::Creators,
            RoyaltyTargetType::Fanout => RoyaltyModel::Fanout,
            RoyaltyTargetType::Single => RoyaltyModel::Single,
            _ => RoyaltyModel::Creators,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Royalty {
    pub royalty_model: RoyaltyModel,
    pub target: Option<String>,
    pub percent: f64,
    pub basis_points: u32,
    pub primary_sale_happened: bool,
    pub locked: bool,
}

pub type Address = String;
pub type Share = String;
pub type Verified = bool;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct TokenAccount {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_amount: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_extensions: Option<Value>,
    pub frozen: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Creator {
    pub address: String,
    pub share: i32,
    pub verified: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Owner {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associated_token_address: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
pub enum OwnershipModel {
    #[serde(rename = "single")]
    Single,
    #[serde(rename = "token")]
    Token,
}

impl From<String> for OwnershipModel {
    fn from(s: String) -> Self {
        match &*s {
            "single" => OwnershipModel::Single,
            "token" => OwnershipModel::Token,
            _ => OwnershipModel::Single,
        }
    }
}

#[cfg(feature = "sql_types")]
impl From<OwnerType> for OwnershipModel {
    fn from(s: OwnerType) -> Self {
        match s {
            OwnerType::Token => OwnershipModel::Token,
            OwnerType::Single => OwnershipModel::Single,
            _ => OwnershipModel::Single,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Ownership {
    pub frozen: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_transferable: Option<bool>,
    pub delegated: bool,
    pub delegate: Option<String>,
    pub ownership_model: OwnershipModel,
    pub owner: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum UseMethod {
    Burn,
    Multiple,
    Single,
}

impl From<String> for UseMethod {
    fn from(s: String) -> Self {
        match &*s {
            "Burn" => UseMethod::Burn,
            "Single" => UseMethod::Single,
            "Multiple" => UseMethod::Multiple,
            _ => UseMethod::Single,
        }
    }
}

pub type Mutability = bool;

impl From<ChainMutability> for Mutability {
    fn from(s: ChainMutability) -> Self {
        match s {
            ChainMutability::Mutable => true,
            ChainMutability::Immutable => false,
            _ => true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Uses {
    pub use_method: UseMethod,
    pub remaining: u64,
    pub total: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct Supply {
    pub print_max_supply: Option<u64>,
    pub print_current_supply: u64,
    pub edition_nonce: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_edition_mint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SystemInfo {
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MplCoreInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_minted: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_size: Option<i32>,
    pub plugins_json_version: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Asset {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_indexed_slot: Option<u64>,
    pub interface: Interface,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorities: Option<Vec<Authority>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression: Option<Compression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grouping: Option<Vec<Group>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub royalty: Option<Royalty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creators: Option<Vec<Creator>>,
    pub ownership: Ownership,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses: Option<Uses>,
    pub supply: Option<Supply>,
    pub mutable: bool,
    pub burnt: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mint_extensions: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_info: Option<TokenInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_definition: Option<GroupDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_plugins: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mpl_core_info: Option<MplCoreInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_plugins: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_external_plugins: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_agent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_signer: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NotFilter {
    #[serde(default)]
    pub collections: Option<Vec<String>>,
    #[serde(default)]
    pub owners: Option<Vec<String>>,
    #[serde(default)]
    pub creators: Option<Vec<String>>,
    #[serde(default)]
    pub authorities: Option<Vec<String>>,
}
