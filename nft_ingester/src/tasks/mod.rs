use self::{
    background_task_listener::BackgroundTaskListener,
    background_task_manager::BackgroundTaskManager, background_task_runner::BackgroundTaskRunner,
};
use crate::{error::IngesterError, metric};
use async_trait::async_trait;
use cadence_macros::is_global_default_set;
use chrono::NaiveDateTime;
use crypto::{digest::Digest, sha2::Sha256};
use plerkle_messenger::MessengerConfig;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::Map;
use sqlx::{Pool, Postgres};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

pub mod background_task_listener;
pub mod background_task_manager;
pub mod background_task_runner;
mod common;
pub use self::common::*;

#[async_trait]
pub trait BgTask: Send + Sync {
    fn name(&self) -> &'static str;
    fn lock_duration(&self) -> i64;
    fn max_attempts(&self) -> i16;
    async fn task(
        &self,
        db: &DatabaseConnection,
        data: serde_json::Value,
        reindex_interval: i64,
        ipfs_gateway: Option<String>,
        ipfs_gateway_token: Option<String>,
        arweave_gateway: Option<String>,
    ) -> Result<(), IngesterError>;
}

const RETRY_INTERVAL: u64 = 1000;
const QUEUE_DEPTH_INTERVAL: u64 = 2500;
const DELETE_INTERVAL: u64 = 30000;
const PURGE_TIME: u64 = 3600;
const TIMEOUT_MS: u64 = 3000;
const MAX_TASK_BATCH_SIZE: usize = 100;
const REINDEX_INTERVAL_MINUTES: u64 = 60;
const RESET_INTERVAL_SECONDS: u64 = 300;
const MAX_CONCURRENT_RUNNER_TASKS: usize = 100;
const MAX_CONCURRENT_MANAGER_TASKS: usize = 100;

pub const BG_TASK_STREAM_KEY: &str = "BG_TASK_STREAM";

#[derive(Deserialize, PartialEq, Debug, Clone)]
pub struct BgTaskConfig {
    pub delete_interval: Option<u64>,
    pub retry_interval: Option<u64>,
    pub query_depth_interval: Option<u64>,
    pub purge_time: Option<u64>,
    pub batch_size: Option<usize>,
    pub lock_duration: Option<i64>,
    pub max_attempts: Option<i16>,
    pub timeout: Option<u64>,
    pub reindex_interval_minutes: Option<u64>,
    pub reset_interval_seconds: Option<u64>,
    pub max_concurrent_runner_tasks: Option<usize>,
    pub max_concurrent_manager_tasks: Option<usize>,
}

impl Default for BgTaskConfig {
    fn default() -> Self {
        BgTaskConfig {
            delete_interval: Some(DELETE_INTERVAL),
            retry_interval: Some(RETRY_INTERVAL),
            query_depth_interval: Some(QUEUE_DEPTH_INTERVAL),
            purge_time: Some(PURGE_TIME),
            batch_size: Some(MAX_TASK_BATCH_SIZE),
            lock_duration: Some(5),
            max_attempts: Some(3),
            timeout: Some(TIMEOUT_MS),
            reindex_interval_minutes: Some(REINDEX_INTERVAL_MINUTES),
            reset_interval_seconds: Some(RESET_INTERVAL_SECONDS),
            max_concurrent_runner_tasks: Some(MAX_CONCURRENT_RUNNER_TASKS),
            max_concurrent_manager_tasks: Some(MAX_CONCURRENT_MANAGER_TASKS),
        }
    }
}

pub struct TaskData {
    pub name: &'static str,
    pub data: serde_json::Value,
    pub created_at: Option<NaiveDateTime>,
}

/// Hashes the task name and data to create a unique identifier for the task.
pub fn hash_task(name: String, data: serde_json::Value) -> Result<String, IngesterError> {
    let mut hasher = Sha256::new();
    let sorted_data = sort_json(&data);
    if let Ok(data) = serde_json::to_vec(&sorted_data) {
        hasher.input(name.as_bytes());
        hasher.input(data.as_slice());
        return Ok(hasher.result_str());
    }
    Err(IngesterError::SerializatonError(
        "Failed to serialize task data".to_string(),
    ))
}

/// Sorts the JSON object by keys so that the hash is consistent.
fn sort_json(json_value: &serde_json::Value) -> serde_json::Value {
    let mut sorted_map = Map::new();
    let map = json_value.as_object();
    match map {
        Some(map) => {
            let ordered: BTreeMap<_, _> = map.iter().collect();
            for (key, value) in ordered {
                sorted_map.insert(key.clone(), sort_json(value));
            }
            sorted_map.into()
        }
        None => json_value.clone(),
    }
}

pub trait FromTaskData<T>: Sized {
    fn from_task_data(data: TaskData) -> Result<T, IngesterError>;
}

pub trait IntoTaskData: Sized {
    fn into_task_data(self) -> Result<TaskData, IngesterError>;
}

/// The `Task` object gets serialized and sent to the Redis stream.
#[derive(Serialize, Deserialize)]
pub struct Task {
    task_type: String,
    data: serde_json::Value,
    attempts: i16,
}

pub struct BackgroundTaskHandler {
    pub manager: BackgroundTaskManager,
    pub listener: BackgroundTaskListener,
    pub runner: BackgroundTaskRunner,
}

impl BackgroundTaskHandler {
    pub async fn new(
        pool: Pool<Postgres>,
        task_defs: Vec<Box<dyn BgTask>>,
        ipfs_gateway: Option<String>,
        ipfs_gateway_token: Option<String>,
        arweave_gateway: Option<String>,
        config: MessengerConfig,
    ) -> Result<Self, IngesterError> {
        let mut tasks = HashMap::new();
        for task in task_defs {
            tasks.insert(task.name().to_string(), task);
        }
        let registered_task_types = Arc::new(tasks);

        let manager = BackgroundTaskManager::new(pool.clone(), config.clone()).await?;
        let listener =
            BackgroundTaskListener::new(pool.clone(), registered_task_types.clone()).await?;
        let runner = BackgroundTaskRunner::new(
            pool,
            registered_task_types.clone(),
            ipfs_gateway,
            ipfs_gateway_token,
            arweave_gateway,
            config,
        )
        .await?;

        Ok(BackgroundTaskHandler {
            manager,
            listener,
            runner,
        })
    }
}
