use config::{load_config, setup_database, Config};
use error::DasJobErr;
use jobs::{
    account_closures_job::handle_missed_account_closures,
    missing_owner_job::fix_missing_owner,
    nft_burns_job::handle_nft_burns_job,
    reindex_job::{reindex_collection, reindex_creator},
};
use log::{debug, error};
use metrics::setup_metrics;
use nft_ingester::config::init_logger;
use plerkle_messenger::Messenger;
use sea_orm::{DatabaseConnection, SqlxPostgresConnector};
use solana_client::nonblocking::rpc_client::RpcClient;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::{
    signal,
    sync::Mutex,
    task::{JoinHandle, JoinSet},
    time::{interval, Duration},
};

pub mod config;
mod error;
mod jobs;
pub mod metrics;

#[tokio::main(flavor = "multi_thread", worker_threads = 16)]
async fn main() -> Result<(), DasJobErr> {
    init_logger(); // Setup logger before doing anything.
    let config = load_config();
    setup_metrics(&config);

    let pool = setup_database(config.database_url.clone(), config.max_db_connections).await;
    let conn: sea_orm::DatabaseConnection =
        SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
    let rpc_url = config.get_rpc_url();
    let client = RpcClient::new(rpc_url.clone());
    let messenger: Arc<Mutex<Box<dyn Messenger>>> = config.get_messenger().await.unwrap();
    let reindex_collections = config.get_reindex_collections().unwrap_or(vec![]);
    let reindex_creators = config.get_reindex_creators().unwrap_or(vec![]);

    let mut tasks = JoinSet::new();
    if let Some(true) = config.enable_fix_owner_job {
        tasks.spawn(start_missing_owner_job(
            client,
            messenger.clone(),
            conn,
            config.clone(),
        ));
    }
    if let Some(true) = config.enable_account_closures_job {
        tasks.spawn(starts_account_closure_job(
            rpc_url.clone(),
            pool.clone(),
            config.clone(),
            config.account_closures_dryrun,
        ));
    }
    if let Some(true) = config.enable_nft_burns_job {
        tasks.spawn(start_burned_nfts_job(
            rpc_url.clone(),
            pool.clone(),
            config.clone(),
        ));
    }
    for collection in reindex_collections {
        let conn: sea_orm::DatabaseConnection =
            SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
        tasks.spawn(start_collection_reindex_job(
            rpc_url.clone(),
            messenger.clone(),
            conn,
            collection.clone(),
            config.clone(),
        ));
    }
    for creator in reindex_creators {
        let conn: sea_orm::DatabaseConnection =
            SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
        tasks.spawn(start_creator_reindex_job(
            rpc_url.clone(),
            messenger.clone(),
            conn,
            creator.clone(),
            config.clone(),
        ));
    }

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
            // we also shut down in case of error
        }
    }
    tasks.shutdown().await;
    Ok(())
}

fn starts_account_closure_job(
    rpc_url: String,
    pool: PgPool,
    config: Config,
    dryrun: bool,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(config.account_closures_interval_ms));
        loop {
            let rpc_client = RpcClient::new(rpc_url.clone());
            let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
            interval.tick().await; // ticks immediately
            let res = handle_missed_account_closures(
                Arc::new(conn),
                Arc::new(rpc_client),
                config.account_closures_max_concurrent_calls,
                config.account_closures_checkpoint.clone(),
                dryrun,
            )
            .await;
            match res {
                Ok(_) => debug!("Successfully ran handle_missed_account_closures job"),
                Err(e) => error!("Error running handle_missed_account_closures job: {:?}", e),
            };
        }
    })
}

fn start_burned_nfts_job(rpc_url: String, pool: PgPool, config: Config) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(config.nft_burns_interval_ms));
        loop {
            let rpc_client = RpcClient::new(rpc_url.clone());
            let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
            interval.tick().await; // ticks immediately
            let res = handle_nft_burns_job(
                Arc::new(conn),
                Arc::new(rpc_client),
                config.nft_burns_max_concurrent_calls,
                config.nft_burns_checkpoint.clone(),
            )
            .await;
            match res {
                Ok(_) => debug!("Successfully ran handle_nft_burns_job job"),
                Err(e) => error!("Error running handle_nft_burns_job job: {:?}", e),
            };
        }
    })
}

/// Spawns the runner tasks
fn start_missing_owner_job(
    client: RpcClient,
    messenger: Arc<Mutex<Box<dyn Messenger>>>,
    conn: DatabaseConnection,
    config: Config,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(config.fix_owner_interval_ms));
        loop {
            interval.tick().await; // ticks immediately
            let res = fix_missing_owner(&conn, &client, &messenger.clone()).await;
            match res {
                Ok(_) => debug!("Successfully ran fix_missing_owner job"),
                Err(e) => error!("Error running fix_missing_owner job: {:?}", e),
            };
        }
    })
}

/// Spawns the runner tasks
fn start_collection_reindex_job(
    rpc_url: String,
    messenger: Arc<Mutex<Box<dyn Messenger>>>,
    conn: DatabaseConnection,
    collection: String,
    config: Config,
) -> JoinHandle<()> {
    let conn = Arc::new(conn);
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(
            config.reindex_collections_interval_ms,
        ));
        loop {
            interval.tick().await; // ticks immediately
            let res =
                reindex_collection(conn.clone(), &collection, &rpc_url, &messenger.clone()).await;
            match res {
                Ok(_) => debug!("Successfully ran collection_reindex job"),
                Err(e) => error!("Error running collection_reindex job: {:?}", e),
            };
        }
    })
}

/// Spawns the runner tasks
fn start_creator_reindex_job(
    rpc_url: String,
    messenger: Arc<Mutex<Box<dyn Messenger>>>,
    conn: DatabaseConnection,
    creator: String,
    config: Config,
) -> JoinHandle<()> {
    let conn = Arc::new(conn);
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(config.reindex_creators_interval_ms));
        loop {
            interval.tick().await; // ticks immediately
            let res = reindex_creator(conn.clone(), &creator, &rpc_url, &messenger.clone()).await;
            match res {
                Ok(_) => debug!("Successfully ran creators_reindex job"),
                Err(e) => error!("Error running creators_reindex job: {:?}", e),
            };
        }
    })
}
