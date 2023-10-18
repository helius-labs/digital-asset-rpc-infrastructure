use digital_asset_types::feature_flag::FeatureFlags;

use crate::config::Config;

pub fn get_feature_flags(config: &Config) -> FeatureFlags {
    FeatureFlags {
        enable_grand_total_query: config.enable_grand_total_query.unwrap_or(true),
        enable_collection_metadata: config.enable_collection_metadata.unwrap_or(true),
    }
}
