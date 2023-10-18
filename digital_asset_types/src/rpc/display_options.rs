use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DisplayOptions {
    #[serde(default)]
    pub show_collection_metadata: bool,
    #[serde(default)]
    pub show_raw_data: bool,
    #[serde(default)]
    pub show_unverified_collections: bool,
    #[serde(default)]
    pub show_grand_total: bool,

    #[serde(skip)]
    pub cdn_prefix: Option<String>,
}
