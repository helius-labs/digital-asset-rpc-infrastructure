use super::redis_task_messenger::{RedisTaskMessenger, StreamTask};
use super::{BgTask, BgTaskConfig};
use crate::tasks::is_global_default_set;
use crate::tasks::BG_TASK_STREAM_KEY;
use crate::{error::IngesterError, metric, tasks::hash_task};
use async_stream::stream;
use cadence_macros::{statsd_count, statsd_histogram};
use chrono::{Duration, Utc};
use digital_asset_types::dao::{offchain_metadata, sea_orm_active_enums::TaskStatus, tasks};
use futures::Stream;
use futures_util::{pin_mut, StreamExt, TryStreamExt};
use lazy_static::lazy_static;
use log::{error, info, warn};
use plerkle_messenger::MessengerConfig;
use regex::Regex;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    sea_query::Expr, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbBackend,
    EntityTrait, QueryFilter, QueryTrait, Set, SqlxPostgresConnector,
};
use sqlx::{Pool, Postgres};
use std::{collections::HashMap, sync::Arc};
use tokio::task::JoinHandle;
use tokio::time::{self};

/// `BackgroundTaskRunner` grabs background tasks from the Redis stream in batches and executes them.
pub struct BackgroundTaskRunner {
    pool: Pool<Postgres>,
    registered_task_types: Arc<HashMap<String, Box<dyn BgTask>>>,
    ipfs_gateway: Option<String>,
    ipfs_gateway_token: Option<String>,
    arweave_gateway: Option<String>,
    messenger: RedisTaskMessenger,
}

impl BackgroundTaskRunner {
    pub async fn new(
        pool: Pool<Postgres>,
        registered_task_types: Arc<HashMap<String, Box<dyn BgTask>>>,
        ipfs_gateway: Option<String>,
        ipfs_gateway_token: Option<String>,
        arweave_gateway: Option<String>,
        config: MessengerConfig,
    ) -> Result<Self, IngesterError> {
        let messenger = RedisTaskMessenger::new(BG_TASK_STREAM_KEY, config).await?;
        Ok(BackgroundTaskRunner {
            pool,
            registered_task_types,
            ipfs_gateway,
            ipfs_gateway_token,
            arweave_gateway,
            messenger,
        })
    }
    /// Start a `BackgroundTaskRunner` which is responsible for running background tasks
    pub fn start(&mut self, config: Option<BgTaskConfig>) -> JoinHandle<()> {
        let config = config.unwrap_or_default();
        let retry_interval = tokio::time::Duration::from_millis(
            config
                .retry_interval
                .unwrap_or(BgTaskConfig::default().retry_interval.unwrap()),
        );

        let reindex_interval = config
            .reindex_interval_minutes
            .unwrap_or(BgTaskConfig::default().reindex_interval_minutes.unwrap());

        let max_concurrent_runner_tasks = config
            .max_concurrent_runner_tasks
            .unwrap_or(BgTaskConfig::default().max_concurrent_runner_tasks.unwrap());

        let batch_size = config
            .batch_size
            .unwrap_or(BgTaskConfig::default().batch_size.unwrap());

        info!(
            "Background task runner config: retry_interval: {:?}, batch_size:{:?}",
            retry_interval, batch_size
        );

        // Get tasks from Redis stream and execute them
        let pool = self.pool.clone();
        let ipfs_gateway = self.ipfs_gateway.clone();
        let ipfs_gateway_token = self.ipfs_gateway_token.clone();
        let arweave_gateway = self.arweave_gateway.clone();
        let task_map = self.registered_task_types.clone();
        let messenger = self.messenger.clone();
        let stream = get_task_stream(messenger.clone(), batch_size, retry_interval);
        tokio::spawn(async move {
            pin_mut!(stream);

            let res = stream
                .map(|result| async {
                    let mut messenger = messenger.clone();
                    match result {
                        Ok(st) => {
                            let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
                            let task_map_clone = task_map.clone();
                            let ipfs_gateway = ipfs_gateway.clone();
                            let ipfs_gateway_token = ipfs_gateway_token.clone();
                            let arweave_gateway = arweave_gateway.clone();
                            let stream_id = st.stream_id.clone();
                            tokio::task::spawn(async move {
                                if let Some(task_executor) =
                                    task_map_clone.clone().get(&*st.task.task_type)
                                {
                                    let res = BackgroundTaskRunner::execute_task(
                                        &conn,
                                        task_executor,
                                        st,
                                        reindex_interval as i64,
                                        ipfs_gateway,
                                        ipfs_gateway_token,
                                        arweave_gateway,
                                    )
                                    .await;
                                    match res {
                                        Ok(_) => return Ok(()),
                                        Err(e) => {
                                            return Err(IngesterError::TaskManagerError(
                                                e.to_string(),
                                            ));
                                        }
                                    }
                                }
                                Err(IngesterError::TaskManagerError(format!(
                                    "{} not a valid task type",
                                    st.task.task_type
                                )))
                            });
                            match messenger.ack(BG_TASK_STREAM_KEY, &vec![stream_id]).await {
                                Ok(_) => Ok(()),
                                Err(e) => Err(IngesterError::TaskManagerError(format!(
                                    "Error while ascking task. Error: {}",
                                    e
                                ))),
                            }
                        }
                        Err(e) => Err(e.into()),
                    }
                })
                .buffer_unordered(max_concurrent_runner_tasks)
                .try_collect::<()>()
                .await;

            if let Err(e) = res {
                error!("Error while getting tassk from async stream. Error: {}", e);
            }
        })
    }

    /// Grab a batch of `Task` objects from the stream and return them.
    async fn get_tasks_from_stream(
        messenger: &RedisTaskMessenger,
        batch_size: usize,
    ) -> Result<Vec<StreamTask>, IngesterError> {
        let mut messenger = messenger.clone();
        let stream_tasks = messenger.recv_tasks(BG_TASK_STREAM_KEY, batch_size).await?;
        Ok(stream_tasks)
    }

    /// Download off-chain metadata and populate DB.
    async fn execute_task(
        db: &DatabaseConnection,
        task_def: &Box<dyn BgTask>,
        stream_task: StreamTask,
        reindex_interval: i64,
        ipfs_gateway: Option<String>,
        ipfs_gateway_token: Option<String>,
        arweave_gateway: Option<String>,
    ) -> Result<(), IngesterError> {
        let task_name = task_def.name();
        let data_json = stream_task.task.data.clone();
        let task_uri = data_json
            .get("uri")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let start = Utc::now();
        let res = task_def
            .task(
                &db,
                data_json,
                reindex_interval,
                ipfs_gateway,
                ipfs_gateway_token,
                arweave_gateway,
            )
            .await;
        let end = Utc::now();
        metric! {
            statsd_histogram!("ingester.bgtask.proc_time", (end.timestamp_millis() - start.timestamp_millis()) as u64, "type" => task_name);
        }

        let task_type = stream_task.task.task_type.clone();
        let task_data = stream_task.task.data.clone();
        let locked_duration = Duration::seconds(task_def.lock_duration());
        let mut task = tasks::ActiveModel {
            id: Set(hash_task(task_type, task_data)?),
            task_type: Set(stream_task.task.task_type),
            data: Set(stream_task.task.data),
            created_at: Set(Utc::now().naive_utc()),
            locked_by: Set(None),
            max_attempts: Set(task_def.max_attempts()),
            duration: Set(None),
            attempts: Set(stream_task.task.attempts),
            ..Default::default()
        };
        match res {
            Ok(_) => {
                task.status = Set(TaskStatus::Success);
                task.locked_until = Set(None);
                task.errors = Set(None);
                BackgroundTaskRunner::save_task(db, task).await?;
                metric! {
                    statsd_count!("ingester.bgtask.success", 1, "type" => task_name);
                }
            }
            Err(e) => {
                task.status = Set(TaskStatus::Failed);
                task.locked_until = Set(Some((Utc::now() + locked_duration).naive_utc()));
                task.attempts = Set(stream_task.task.attempts + 1);
                task.errors = Set(Some(e.to_string()));

                match e {
                    IngesterError::UnrecoverableTaskError(_) => {
                        task.attempts = Set(task_def.max_attempts() + 1);
                        task.locked_by = Set(Some("permanent failure".to_string()));
                    }
                    _ => {}
                }
                BackgroundTaskRunner::save_task(db, task).await?;

                match e {
                    IngesterError::BatchInitNetworkingError(msg) => {
                        // Network errors are common for off-chain JSONs.
                        // Logging these as errors is far too noisy.
                        metric! {
                            statsd_count!("ingester.bgtask.network_error", 1, "type" => task_name);
                        }
                        warn!("Task failed due to network error: {}", msg);
                        if task_name == "DownloadMetadata" {
                            if let Some(ref uri) = task_uri {
                                BackgroundTaskRunner::mark_offchain_transient_failure(db, uri)
                                    .await;
                            }
                        }
                    }
                    IngesterError::HttpError {
                        ref status_code,
                        ref uri,
                    } => {
                        metric! {
                            statsd_count!("ingester.bgtask.http_error", 1,
                                "status" => status_code,
                                "type" => task_name);
                        }
                        let root_domain =
                            BackgroundTaskRunner::get_root_domain(uri).unwrap_or_default();
                        metric! {
                            statsd_count!("ingester.bgtask.http_error", 1,
                                "status" => status_code,
                                "type" => task_name,
                                "root_domain" => root_domain.as_str());
                        }
                        warn!("Task failed due to HTTP error: {}", e);

                        // A gone/unpurchasable status earns a permanent-failure record so
                        // the URI is probed once per horizon instead of once per touch.
                        // Rate limiting and bot blocking stay retryable.
                        if task_name == "DownloadMetadata" {
                            if let Some(ref uri) = task_uri {
                                if crate::tasks::common::is_permanent_http_status(status_code) {
                                    let err_msg = e.to_string();
                                    if let Err(db_err) =
                                        BackgroundTaskRunner::mark_offchain_permanent_failure(
                                            db,
                                            uri,
                                            &err_msg,
                                            Some(status_code),
                                        )
                                        .await
                                    {
                                        warn!(
                                            "Failed to mark offchain permanent failure for {}: {}",
                                            uri, db_err
                                        );
                                    }
                                } else {
                                    // Retryable statuses (429, 403, 5xx) stamp the
                                    // probe time so touch-driven re-arms respect the
                                    // retry horizon instead of retrying per touch.
                                    BackgroundTaskRunner::mark_offchain_transient_failure(db, uri)
                                        .await;
                                }
                            }
                        }
                    }
                    IngesterError::UnrecoverableTaskError(_) => {
                        metric! {
                            statsd_count!("ingester.bgtask.unrecoverable_error", 1, "type" => task_name);
                        }
                        warn!("{}", e);

                        // Mark offchain_metadata so the daily cron (which filters
                        // metadata='processing') won't keep re-creating this task,
                        // and should_reindex will skip it within the grace period.
                        if task_name == "DownloadMetadata" {
                            if let Some(ref uri) = task_uri {
                                let err_msg = e.to_string();
                                if let Err(db_err) =
                                    BackgroundTaskRunner::mark_offchain_permanent_failure(
                                        db, uri, &err_msg, None,
                                    )
                                    .await
                                {
                                    warn!(
                                        "Failed to mark offchain permanent failure for {}: {}",
                                        uri, db_err
                                    );
                                }
                            }
                        }
                    }
                    _ => {
                        metric! {
                            statsd_count!("ingester.bgtask.error", 1, "type" => task_name);
                        }
                        error!("Task Run Error: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Save a task in the `tasks` table.
    async fn save_task<A>(db: &A, task: tasks::ActiveModel) -> Result<(), IngesterError>
    where
        A: ConnectionTrait,
    {
        let query = tasks::Entity::insert(task)
            .on_conflict(
                OnConflict::columns([tasks::Column::Id])
                    .update_columns([
                        tasks::Column::Status,
                        tasks::Column::LockedUntil,
                        tasks::Column::LockedBy,
                        tasks::Column::Attempts,
                        tasks::Column::Errors,
                    ])
                    .to_owned(),
            )
            .build(DbBackend::Postgres);
        db.execute(query).await.map(|_| ()).map_err(|e| e.into())
    }

    /// Stamps the probe time on a still-unfetched row after a retryable
    /// failure (timeout, 429, 5xx). The document and reindex flag are left
    /// alone; the timestamp only feeds the touch-driven re-arm horizon in
    /// guard_offchain_insert_repair, bounding dead-link probes to one per
    /// horizon instead of one per account touch.
    ///
    /// A row already stamped within the last hour is left alone. Every task
    /// attempt reaches this path, and during a gateway outage that is
    /// millions of attempts against a table whose rows are wide; one stamp
    /// per hour per URI carries the same horizon at a fraction of the WAL.
    ///
    /// Best-effort: a miss here only means an earlier re-probe.
    async fn mark_offchain_transient_failure(db: &DatabaseConnection, uri: &str) {
        let now = chrono::Utc::now().fixed_offset();
        if let Err(db_err) = offchain_metadata::Entity::update_many()
            .col_expr(offchain_metadata::Column::UpdatedAt, Expr::value(now))
            .filter(offchain_metadata::Column::MetadataUrl.eq(uri))
            .filter(
                offchain_metadata::Column::Metadata
                    .eq(serde_json::Value::String("processing".to_string())),
            )
            .filter(Expr::cust(
                "(offchain_metadata.updated_at IS NULL
                  OR offchain_metadata.updated_at < now() - interval '1 hour')",
            ))
            .exec(db)
            .await
        {
            warn!("Failed to stamp transient failure for {}: {}", uri, db_err);
        }
    }

    /// On permanent failure, replace metadata='processing' with an error value so
    /// the daily cron (which filters metadata='processing') stops re-creating this task.
    /// Only overwrites metadata that is still 'processing' — never clobber real metadata.
    async fn mark_offchain_permanent_failure(
        db: &DatabaseConnection,
        uri: &str,
        error_msg: &str,
        status_code: Option<&str>,
    ) -> Result<(), IngesterError> {
        let now = chrono::Utc::now().fixed_offset();
        let error_metadata = serde_json::json!({
            "error": "permanent_failure",
            "msg": error_msg,
            "code": status_code,
        });
        offchain_metadata::Entity::update_many()
            .col_expr(
                offchain_metadata::Column::Metadata,
                Expr::value(error_metadata),
            )
            .col_expr(offchain_metadata::Column::Reindex, Expr::value(false))
            .col_expr(offchain_metadata::Column::UpdatedAt, Expr::value(now))
            .filter(offchain_metadata::Column::MetadataUrl.eq(uri))
            // Guard against overwriting a successfully fetched document, but do
            // refresh an existing permanent-failure marker: a horizon-driven
            // re-probe that fails again must push updated_at forward, otherwise
            // the row stays "stale" and is re-probed on every account touch.
            .filter(
                Condition::any()
                    .add(
                        offchain_metadata::Column::Metadata
                            .eq(serde_json::Value::String("processing".to_string())),
                    )
                    .add(Expr::cust(
                        "offchain_metadata.metadata->>'error' = 'permanent_failure'",
                    )),
            )
            .exec(db)
            .await
            .map(|_| ())
            .map_err(|e| e.into())
    }

    /// Extracts root domain from a URI.
    /// Used to provide additional dimension to our offchain failure metrics.
    fn get_root_domain(uri: &str) -> Option<String> {
        lazy_static! {
            static ref ROOT_DOMAIN_REGEX: Regex =
                Regex::new(r"https?://(?:[a-zA-Z0-9-]+\.)?([a-zA-Z0-9-]+\.[a-zA-Z]{2,})(?::\d+)?/")
                    .unwrap();
        }
        if let Some(captures) = ROOT_DOMAIN_REGEX.captures(&uri) {
            Some(captures[1].to_string())
        } else {
            None
        }
    }
}

fn get_task_stream(
    messenger: RedisTaskMessenger,
    batch_size: usize,
    interval: tokio::time::Duration,
) -> impl Stream<Item = Result<StreamTask, IngesterError>> {
    let mut interval = time::interval(interval);
    let stream = stream! {
        loop {
            interval.tick().await; // ticks immediately
            let res = BackgroundTaskRunner::get_tasks_from_stream(&messenger, batch_size).await;
            match res {
                Ok(stream_tasks) => {
                    for st in stream_tasks {
                        yield Ok(st);
                    }
                }
                Err(e) => {
                    yield Err(e.into());
                }
            }
        }
    };
    stream
}
