use common::config::load_config_using_env_prefix;

use serde::Deserialize;

#[derive(Deserialize, Clone, Default)]
pub struct Config {
    pub rpc_url: String,
    pub database_urls: Option<String>,
    pub metrics_port: Option<u16>,
    pub metrics_host: Option<String>,
    pub server_port: u16,
    pub env: Option<String>,
    pub cdn_prefix: Option<String>,
    pub collection_list: Option<String>,
    pub creators_list: Option<String>,
    pub authority_list: Option<String>,
    pub db_work_mem: Option<String>,
    pub db_max_conn: Option<u32>,
    pub enable_grand_total_query: Option<bool>,
    pub enable_collection_metadata: Option<bool>,
    pub enable_search_assets_negation: Option<bool>,
    pub verify_asset_exists: Option<bool>,
    pub region: Option<String>,
}

pub fn load_config() -> Config {
    load_config_using_env_prefix("APP_")
}
