use crate::config::Config;

pub struct FeatureFlags {
    pub enable_grand_total_query: bool,
    pub enable_grouping_metadata: bool,
}

pub fn get_feature_flags(config: &Config) -> FeatureFlags {
    FeatureFlags {
        enable_grand_total_query: config.enable_grand_total_query.unwrap_or(false),
        enable_grouping_metadata: config.enable_grouping_metadata.unwrap_or(false),
    }
}
