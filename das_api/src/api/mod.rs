use crate::error::DasApiError;
use async_trait::async_trait;
use digital_asset_types::dao::scopes::asset::TokenType;
use digital_asset_types::dao::CreatedAtFilter;
use digital_asset_types::rpc::filter::AssetSortDirection;
use digital_asset_types::rpc::filter::AssetSorting;
use digital_asset_types::rpc::filter::SearchConditionType;
use digital_asset_types::rpc::options::SearchAssetsOptions;
use digital_asset_types::rpc::options::{GetAssetOptions, Options};
use digital_asset_types::rpc::response::{
    AssetList, EditionsList, OwnerList, TokenAccountsList, TransactionSignatureList,
};
use digital_asset_types::rpc::{
    Asset, AssetProof, Interface, NotFilter, OwnershipModel, RoyaltyModel,
};
use open_rpc_derive::{document_rpc, rpc};
use open_rpc_schema::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod api_impl;
pub use api_impl::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAssetsByGroup {
    pub group_key: String,
    pub group_value: String,
    pub sort_by: Option<AssetSorting>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(default, alias = "displayOptions")]
    pub options: Option<Options>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAssetsByOwner {
    pub owner_address: String,
    pub sort_by: Option<AssetSorting>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(default, alias = "displayOptions")]
    pub options: Option<Options>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAsset {
    pub id: String,
    #[serde(default)]
    pub raw_data: Option<bool>, // TODO: Deprecate
    #[serde(default, alias = "displayOptions")]
    pub options: Option<GetAssetOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAssets {
    pub ids: Vec<String>,
    #[serde(default, alias = "displayOptions")]
    pub options: Option<GetAssetOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAssetProof {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAssetProofs {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAssetsByCreator {
    pub creator_address: String,
    pub only_verified: Option<bool>,
    pub sort_by: Option<AssetSorting>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(default, alias = "displayOptions")]
    pub options: Option<Options>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SearchAssets {
    pub negate: Option<bool>,
    pub condition_type: Option<SearchConditionType>,
    pub interface: Option<Interface>,
    pub owner_address: Option<String>,
    pub owner_type: Option<OwnershipModel>,
    pub creator_address: Option<String>,
    pub creator_verified: Option<bool>,
    pub authority_address: Option<String>,
    pub grouping: Option<(String, String)>,
    pub delegate: Option<String>,
    pub frozen: Option<bool>,
    pub supply: Option<u64>,
    pub supply_mint: Option<String>,
    pub compressed: Option<bool>,
    pub compressible: Option<bool>,
    pub royalty_target_type: Option<RoyaltyModel>,
    pub royalty_target: Option<String>,
    pub royalty_amount: Option<u32>,
    pub burnt: Option<bool>,
    pub sort_by: Option<AssetSorting>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(default)]
    pub json_uri: Option<String>,
    #[serde(default)]
    pub not: Option<NotFilter>,
    #[serde(default, alias = "displayOptions")]
    pub options: Option<SearchAssetsOptions>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub collections: Option<Vec<String>>,
    #[serde(default)]
    pub token_type: Option<TokenType>,
    #[serde(default)]
    pub created_at: Option<CreatedAtFilter>,
    #[serde(default)]
    pub tree: Option<String>,
    #[serde(default)]
    pub collection_nft: Option<bool>,
    #[serde(default, alias = "isAgent")]
    pub is_agent: Option<bool>,
    #[serde(default, alias = "agentToken")]
    pub agent_token: Option<String>,
    #[serde(default, alias = "assetSigner")]
    pub asset_signer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAssetsByAuthority {
    pub authority_address: String,
    pub sort_by: Option<AssetSorting>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(default, alias = "displayOptions")]
    pub options: Option<Options>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetGrouping {
    pub group_key: String,
    pub group_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAssetSignatures {
    pub id: Option<String>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub tree: Option<String>,
    pub leaf_index: Option<i64>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub sort_direction: Option<AssetSortDirection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SearchOwners {
    pub asset: String,
    pub limit: Option<u32>,
    pub page: Option<u32>,
    #[serde(default, alias = "displayOptions")]
    pub options: Option<Options>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetTokenAccounts {
    pub owner: Option<String>,
    pub mint: Option<String>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(default, alias = "displayOptions")]
    pub options: Option<Options>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetNftEditions {
    pub mint: Option<String>,
    pub limit: Option<u32>,
    pub page: Option<u32>,
}

#[document_rpc]
#[async_trait]
pub trait ApiContract: Send + Sync + 'static {
    // Legacy health check.
    async fn check_health(&self) -> Result<(), DasApiError>;

    // Kubernetes health checks.
    async fn liveness(&self) -> Result<(), DasApiError>;
    async fn readiness(&self) -> Result<(), DasApiError>;

    #[rpc(
        name = "getAssetProof",
        params = "named",
        summary = "Get a merkle proof for a compressed asset by its ID"
    )]
    async fn get_asset_proof(&self, payload: GetAssetProof) -> Result<AssetProof, DasApiError>;
    #[rpc(
        name = "getAssetProofs",
        params = "named",
        summary = "Get merkle proofs for compressed assets by their IDs"
    )]
    async fn get_asset_proofs(
        &self,
        payload: GetAssetProofs,
    ) -> Result<HashMap<String, Option<AssetProof>>, DasApiError>;
    #[rpc(
        name = "getAsset",
        params = "named",
        summary = "Get an asset by its ID"
    )]
    async fn get_asset(&self, payload: GetAsset) -> Result<Asset, DasApiError>;
    #[rpc(
        name = "getAssets",
        params = "named",
        summary = "Get assets by their IDs"
    )]
    async fn get_assets(&self, payload: GetAssets) -> Result<Vec<Option<Asset>>, DasApiError>;
    #[rpc(
        name = "getAssetsV2",
        params = "named",
        summary = "Get assets by their IDs"
    )]
    async fn get_assets_v2(&self, payload: GetAssets) -> Result<digital_asset_types::rpc::response::GetAssetsV2Response, DasApiError>;
    async fn get_assets_by_owner(
        &self,
        payload: GetAssetsByOwner,
    ) -> Result<AssetList, DasApiError>;
    #[rpc(
        name = "getAssetsByGroup",
        params = "named",
        summary = "Get a list of assets by a group key and value"
    )]
    async fn get_assets_by_group(
        &self,
        payload: GetAssetsByGroup,
    ) -> Result<AssetList, DasApiError>;
    #[rpc(
        name = "getAssetsByCreator",
        params = "named",
        summary = "Get a list of assets created by an address"
    )]
    async fn get_assets_by_creator(
        &self,
        payload: GetAssetsByCreator,
    ) -> Result<AssetList, DasApiError>;
    #[rpc(
        name = "getAssetsByAuthority",
        params = "named",
        summary = "Get a list of assets with a specific authority"
    )]
    async fn get_assets_by_authority(
        &self,
        payload: GetAssetsByAuthority,
    ) -> Result<AssetList, DasApiError>;
    #[rpc(
        name = "searchAssets",
        params = "named",
        summary = "Search for assets by a variety of parameters"
    )]
    async fn search_assets(&self, payload: SearchAssets) -> Result<AssetList, DasApiError>;
    #[rpc(
        name = "getAssetSignatures",
        params = "named",
        summary = "Get transaction signatures for an asset"
    )]
    async fn get_asset_signatures(
        &self,
        payload: GetAssetSignatures,
    ) -> Result<TransactionSignatureList, DasApiError>;
    #[rpc(
        name = "searchOwners",
        params = "named",
        summary = "Get all owners of an asset sorted by the amount they own"
    )]
    async fn search_owners(&self, payload: SearchOwners) -> Result<OwnerList, DasApiError>;
    #[rpc(
        name = "getTokenAccounts",
        params = "named",
        summary = "Get all token accounts for an owner or a mint"
    )]
    async fn get_token_accounts(
        &self,
        payload: GetTokenAccounts,
    ) -> Result<TokenAccountsList, DasApiError>;
    #[rpc(
        name = "getNftEditions",
        params = "named",
        summary = "Get all editions for a nft mint"
    )]
    async fn get_nft_editions(&self, payload: GetNftEditions) -> Result<EditionsList, DasApiError>;
}
