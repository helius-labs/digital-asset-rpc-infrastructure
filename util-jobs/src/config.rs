use crate::error::DasJobErr;
use common::config::load_config_using_env_prefix;
use plerkle_messenger::{Messenger, MessengerConfig, ACCOUNT_STREAM};
use serde::Deserialize;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub metrics_port: Option<u16>,
    pub metrics_host: Option<String>,
    pub rpc_url: String,
    pub database_url: String,
    pub max_db_connections: Option<u32>,
    pub messenger_config: MessengerConfig,

    // Missing Owners Job
    pub enable_fix_owner_job: Option<bool>,
    pub fix_owner_interval_ms: u64,

    // Reindex Collections Job
    pub reindex_collections: Option<Vec<String>>,
    pub reindex_collections_interval_ms: u64,

    // Reindex Creators Job
    pub reindex_creators: Option<Vec<String>>,
    pub reindex_creators_interval_ms: u64,

    // Account Closures Job
    pub enable_account_closures_job: Option<bool>,
    pub account_closures_interval_ms: u64,
    pub account_closures_checkpoint: Option<String>,
    pub account_closures_max_concurrent_calls: usize,
    pub account_closures_dryrun: bool,

    // NFT Burns job
    pub enable_nft_burns_job: Option<bool>,
    pub nft_burns_interval_ms: u64,
    pub nft_burns_checkpoint: Option<String>,
    pub nft_burns_max_concurrent_calls: usize,
}

pub const DATABASE_URL_KEY: &str = "url";
pub const RPC_URL_KEY: &str = "url";

impl Config {
    pub fn get_database_url(&self) -> String {
        self.database_url.clone()
    }

    pub fn get_rpc_url(&self) -> String {
        self.rpc_url.clone()
    }

    pub async fn get_messenger(&self) -> Result<Arc<Mutex<Box<dyn Messenger>>>, DasJobErr> {
        let mut messenger = plerkle_messenger::select_messenger(self.messenger_config.to_owned())
            .await
            .unwrap();
        messenger.add_stream(ACCOUNT_STREAM).await.unwrap();
        messenger
            .set_buffer_size(ACCOUNT_STREAM, 10000000000000000)
            .await;
        let messenger = Arc::new(Mutex::new(messenger));
        Ok(messenger)
    }

    pub fn get_reindex_collections(&self) -> Option<Vec<String>> {
        self.reindex_collections.clone()
    }

    pub fn get_reindex_creators(&self) -> Option<Vec<String>> {
        self.reindex_creators.clone()
    }
}

pub fn load_config() -> Config {
    load_config_using_env_prefix("JOB_")
}

pub async fn setup_database(database_url: String, max_db_connections: Option<u32>) -> PgPool {
    let options: PgConnectOptions = database_url.parse().unwrap();
    PgPoolOptions::new()
        .min_connections(1)
        .max_connections(max_db_connections.unwrap_or(10))
        .connect_with(options)
        .await
        .unwrap()
}
