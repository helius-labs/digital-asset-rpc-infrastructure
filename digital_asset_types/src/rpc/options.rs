use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Options {
    // Customer configured
    #[serde(default)]
    pub show_collection_metadata: bool,
    #[serde(default)]
    pub show_grand_total: bool,
    #[serde(default)]
    pub show_unverified_collections: bool,
    #[serde(default)]
    pub show_raw_data: bool,
    #[serde(default)]
    pub show_fungible: bool,
    #[serde(default)]
    pub require_full_index: bool,
    #[serde(default)]
    pub show_system_metadata: bool,
    #[serde(default)]
    pub show_zero_balance: bool,
    #[serde(default)]
    pub show_closed_accounts: bool,

    #[serde(default)]
    pub show_native_balance: bool,
    #[serde(default)]
    pub show_inscription: bool,

    // Not configured by the customer
    #[serde(skip)]
    pub cdn_prefix: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GetAssetOptions {
    // Customer configured
    #[serde(default)]
    pub show_collection_metadata: bool,
    #[serde(default)]
    pub show_unverified_collections: bool,
    #[serde(default)]
    pub show_raw_data: bool,
    #[serde(default)]
    pub show_fungible: bool,
    #[serde(default)]
    pub require_full_index: bool,
    #[serde(default)]
    pub show_system_metadata: bool,

    // These are options that are used by helius-router in CF.
    // We keep this option to indicate that DAS does support this but the support is
    // not implemented within the API.
    #[serde(default)]
    pub show_native_balance: bool,
    #[serde(default)]
    pub show_inscription: bool,

    // Not configured by the customer
    #[serde(skip)]
    pub cdn_prefix: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SearchAssetsOptions {
    // Customer configured
    #[serde(default)]
    pub show_collection_metadata: bool,
    #[serde(default)]
    pub show_grand_total: bool,
    #[serde(default)]
    pub show_unverified_collections: bool,
    #[serde(default)]
    pub show_raw_data: bool,
    #[serde(default)]
    pub require_full_index: bool,
    #[serde(default)]
    pub show_system_metadata: bool,
    #[serde(default)]
    pub show_zero_balance: bool,
    #[serde(default)]
    pub show_closed_accounts: bool,

    #[serde(default)]
    pub show_native_balance: bool,
    #[serde(default)]
    pub show_inscription: bool,

    // Not configured by the customer
    #[serde(skip)]
    pub cdn_prefix: Option<String>,
}

impl From<GetAssetOptions> for Options {
    fn from(o: GetAssetOptions) -> Options {
        Options {
            show_collection_metadata: o.show_collection_metadata,
            show_unverified_collections: o.show_unverified_collections,
            show_raw_data: o.show_raw_data,
            show_grand_total: false,
            cdn_prefix: o.cdn_prefix,
            show_fungible: o.show_fungible,
            require_full_index: o.require_full_index,
            show_system_metadata: o.show_system_metadata,

            // No-ops. Don't remove though
            show_native_balance: false,
            show_inscription: false,
            show_zero_balance: false,
            show_closed_accounts: false,
        }
    }
}

impl From<SearchAssetsOptions> for Options {
    fn from(o: SearchAssetsOptions) -> Options {
        Options {
            show_collection_metadata: o.show_collection_metadata,
            show_unverified_collections: o.show_unverified_collections,
            show_raw_data: o.show_raw_data,
            show_grand_total: o.show_grand_total,
            cdn_prefix: o.cdn_prefix,
            require_full_index: o.require_full_index,
            show_system_metadata: o.show_system_metadata,
            show_native_balance: o.show_native_balance,
            show_inscription: o.show_inscription,
            show_zero_balance: o.show_zero_balance,
            show_closed_accounts: o.show_closed_accounts,

            // TODO: making this no-op because we use TokenType for searchAssets.
            // But best option would be to use TokenType for both getAssetsByOwner and searchAssets and deprecate this.
            show_fungible: false,
        }
    }
}
