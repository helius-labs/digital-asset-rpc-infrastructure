mod account_updates;
mod ack;
mod backfiller;
pub mod config;
mod database;
pub mod error;
pub mod metrics;
mod program_transformers;
mod stream;
pub mod tasks;
mod transaction_notifications;

use crate::{
    account_updates::account_worker,
    ack::ack_worker,
    backfiller::setup_backfiller,
    config::{init_logger, setup_ingester_config, IngesterRole, PodType},
    database::setup_database,
    error::IngesterError,
    metrics::setup_metrics,
    stream::StreamSizeTimer,
    tasks::{price::PriceTaskManager, BackgroundTaskHandler, BgTask, DownloadMetadataTask},
    transaction_notifications::transaction_worker,
};
use cadence_macros::{is_global_default_set, statsd_count};
use chrono::Duration;
use log::{error, info};
use plerkle_messenger::{
    redis_messenger::RedisMessenger, ConsumptionType, ACCOUNT_STREAM, ACC_BACKFILL,
    TRANSACTION_STREAM,
};
use std::{sync::Arc, time};
use tokio::{signal, task::JoinSet};

#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> Result<(), IngesterError> {
    init_logger();
    info!("Starting nft_ingester");

    // Setup Configuration and Metrics ---------------------------------------------

    // Pull Env variables into config struct
    let config = setup_ingester_config();

    // Optionally setup metrics if config demands it
    setup_metrics(&config);

    // One pool many clones, this thing is thread safe and send sync
    let database_pool = setup_database(&config.clone()).await;

    // The role determines the processes that get run.
    let role = config.role.clone().unwrap_or(IngesterRole::All);

    //The pod_type determines the type of pod the ingester is running in
    let pod_type = config.pod_type.clone().unwrap_or(PodType::Regular);

    info!("Starting Program with Role: {}", role);
    info!("Starting Program with Pod Type: {}", pod_type);

    // Tasks Setup -----------------------------------------------
    // This joinSet manages all the tasks that are spawned.
    let mut tasks = JoinSet::new();

    // BACKGROUND TASKS --------------------------------------------
    //Setup definitions for background tasks
    let task_runner_config = config.bg_task_config.clone().unwrap_or_default();
    let bg_task_definitions: Vec<Box<dyn BgTask>> = vec![Box::new(DownloadMetadataTask {
        lock_duration: task_runner_config.lock_duration,
        max_attempts: task_runner_config.max_attempts,
        timeout: task_runner_config
            .timeout
            .map(|t| time::Duration::from_millis(t)),
    })];

    let mut background_task_handler = BackgroundTaskHandler::new(
        database_pool.clone(),
        bg_task_definitions,
        config.ipfs_gateway.clone(),
        config.ipfs_gateway_token.clone(),
        config.arweave_gateway.clone(),
        config.get_messenger_client_config(),
    )
    .await?;

    let price_task_manager = Arc::new(PriceTaskManager::new(database_pool.clone()));

    let bg_task_config = config.bg_task_config.clone();
    if role != IngesterRole::BackgroundTaskRunner && role != IngesterRole::BackgroundTaskManager {
        tasks.spawn(background_task_handler.listener.start());
    }
    if role == IngesterRole::BackgroundTaskRunner || role == IngesterRole::All {
        tasks.spawn(background_task_handler.runner.start(bg_task_config.clone()));
    }
    if role == IngesterRole::BackgroundTaskManager || role == IngesterRole::All {
        tasks.spawn(background_task_handler.manager.start(bg_task_config));
        tasks.spawn(price_task_manager.start_runner(config.clone().price_update_interval_seconds));
    }

    // Stream Size Timers ----------------------------------------
    // Setup Stream Size Timers, these are small processes that run every 30 seconds and farm metrics for the size of the streams.
    // If metrics are disabled, these will not run.
    let stream_metrics_timer = Duration::seconds(30).to_std().unwrap();

    let mut timer_acc = StreamSizeTimer::new(
        stream_metrics_timer,
        config.messenger_config.clone(),
        match pod_type {
            PodType::Backfiller => ACC_BACKFILL,
            PodType::Regular => ACCOUNT_STREAM,
        },
    )?;
    let mut timer_txn = StreamSizeTimer::new(
        stream_metrics_timer,
        config.messenger_config.clone(),
        TRANSACTION_STREAM,
    )?;
    if let Some(t) = timer_acc.start::<RedisMessenger>().await {
        tasks.spawn(t);
    }
    if let Some(t) = timer_txn.start::<RedisMessenger>().await {
        tasks.spawn(t);
    }

    // Stream Consumers Setup -------------------------------------
    if role == IngesterRole::Ingester || role == IngesterRole::All {
        let bg_task_sender = background_task_handler.listener.get_sender().unwrap();
        let (_ack_task, ack_sender) =
            ack_worker::<RedisMessenger>(config.get_messenger_client_config());
        for i in 0..config.get_account_stream_worker_count() {
            let _account = account_worker::<RedisMessenger>(
                database_pool.clone(),
                config.clone(),
                bg_task_sender.clone(),
                ack_sender.clone(),
                if i == 0 {
                    ConsumptionType::Redeliver
                } else {
                    ConsumptionType::New
                },
                &pod_type,
            );
        }
        for i in 0..config.get_transaction_stream_worker_count() {
            let _txn = transaction_worker::<RedisMessenger>(
                database_pool.clone(),
                config.clone(),
                bg_task_sender.clone(),
                ack_sender.clone(),
                if i == 0 {
                    ConsumptionType::Redeliver
                } else {
                    ConsumptionType::New
                },
            );
        }
    }

    // Backfiller Setup ------------------------------------------
    if role == IngesterRole::Backfiller || role == IngesterRole::All {
        let backfiller = setup_backfiller::<RedisMessenger>(database_pool.clone(), config.clone());
        tasks.spawn(backfiller);
    }

    let roles_str = role.to_string();
    metric! {
        statsd_count!("ingester.startup", 1, "role" => &roles_str);
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
