use std::{env, sync::Arc, time::Duration};

use das_publisher::{
    fetchers::{self, GrpcConfig},
    monitor,
    publisher::{load_messenger, publish_block_stream},
    utils,
};
use fetchers::load_block_stream;
use monitor::{
    continously_monitor_das, fetch_current_slot_with_infinite_retry,
    fetch_last_indexed_optional_slot_with_infinite_retry,
};
use nft_ingester::{
    config::setup_ingester_config, database::setup_database, metrics::setup_metrics,
};
use sea_orm::SqlxPostgresConnector;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;

const NUM_MESSENGERS: usize = 10;

fn setup_logging() {
    let env_filter = env::var("RUST_LOG")
        .unwrap_or("info,sqlx=error,sea_orm_migration=error,jsonrpsee_server=warn".to_string());
    let subscriber = tracing_subscriber::fmt().with_env_filter(env_filter);
    subscriber.json().init();
}

use clap::Parser;
use tokio::time::sleep;
use utils::fetch_block_parent_slot;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Optional start slot to begin indexing from. Use "latest" to start from the latest slot.
    #[arg(long, short)]
    start_slot: Option<String>,
}

pub async fn get_genesis_hash_with_infinite_retry(rpc_client: &RpcClient) -> String {
    loop {
        match rpc_client.get_genesis_hash().await {
            Ok(genesis_hash) => return genesis_hash.to_string(),
            Err(e) => {
                log::error!("Failed to fetch genesis hash: {}", e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

pub async fn get_network_start_slot(rpc_client: &RpcClient) -> u64 {
    let genesis_hash = get_genesis_hash_with_infinite_retry(rpc_client).await;
    match genesis_hash.as_str() {
        // Devnet
        "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG" => 341362033,
        // Mainnet
        "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d" => 302433207,
        _ => 0,
    }
}

#[tokio::main]
async fn main() {
    let ingester_config = setup_ingester_config();
    let pool = setup_database(&ingester_config).await;
    setup_metrics(&ingester_config);
    setup_logging();

    let rpc_url = ingester_config.get_rpc_url();
    let rpc_client = Arc::new(RpcClient::new_with_timeout_and_commitment(
        rpc_url,
        Duration::from_secs(90),
        CommitmentConfig::confirmed(),
    ));

    let db = SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
    let args = Args::parse();
    let start_slot = args.start_slot;
    let last_indexed_slot = match start_slot {
        Some(start_slot) => match start_slot == "latest" {
            true => fetch_current_slot_with_infinite_retry(&rpc_client).await,
            false => {
                let start_slot = start_slot.parse::<u64>().unwrap();
                fetch_block_parent_slot(&rpc_client, start_slot).await
            }
        },
        None => match fetch_last_indexed_optional_slot_with_infinite_retry(&db).await {
            Some(slot) => slot,
            None => fetch_current_slot_with_infinite_retry(&rpc_client).await,
        },
    };
    let monitor_handle = continously_monitor_das(db, rpc_client.clone());
    let messenger_config = ingester_config.get_messenger_client_config();
    let mut messenger_pool = Vec::new();
    for _ in 0..NUM_MESSENGERS {
        messenger_pool.push(load_messenger(messenger_config.clone()).await.unwrap());
    }
    let grpc_config = ingester_config.grpc_url.map(|url| GrpcConfig {
        url,
        auth_header: ingester_config.grpc_x_token.unwrap(),
    });
    let block_stream = load_block_stream(rpc_client.clone(), grpc_config, last_indexed_slot);
    let db = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
    publish_block_stream(
        block_stream,
        db,
        messenger_pool,
        rpc_client,
        last_indexed_slot,
    )
    .await;
    monitor_handle.await.unwrap();
}
