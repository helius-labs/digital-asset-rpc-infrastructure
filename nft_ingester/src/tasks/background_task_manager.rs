use super::redis_task_messenger::RedisTaskMessenger;
use super::{BgTaskConfig, TaskData};
use crate::tasks::{is_global_default_set, Task, BG_TASK_STREAM_KEY};
use crate::{error::IngesterError, metric};
use async_stream::stream;
use cadence_macros::{statsd_count, statsd_gauge};
use chrono::Utc;
use digital_asset_types::dao::sea_orm_active_enums::TaskStatusEnum;
use digital_asset_types::dao::{sea_orm_active_enums::TaskStatus, tasks};
use futures::Stream;
use futures_util::{pin_mut, StreamExt, TryStreamExt};
use log::{debug, error, info};
use plerkle_messenger::MessengerConfig;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, DeleteResult, EntityTrait, Order, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, SqlxPostgresConnector, UpdateResult,
};
use sqlx::{Pool, Postgres};
use tokio::time;
use tokio::{
    sync::mpsc::{self, UnboundedSender},
    task::JoinHandle,
};

/*
 * TaskStatus::Success -> Tasks that succeeded successfully.
 * TaskStatus::Failed  -> Tasks that failed. Can be retried until max_attempts.
 * TaskStatus::Pending -> Tasks that haven't been sent to the stream yet.
 * TaskStatus::Running -> Tasks that have been sent to the stream but haven't been marked as Success / Failed yet.
*/

/// `BackgroundTaskManager` is responsible for polling the following:
/// 1. Polling the `tasks` table continuously for tasks and pushing them to Redis stream.
/// 2. Emit different metrics for queue depth periodically.
/// 3. Purge old tasks from the `tasks` table periodically.
pub struct BackgroundTaskManager {
    pool: Pool<Postgres>,
    producer: Option<UnboundedSender<TaskData>>,
    messenger: RedisTaskMessenger,
}

impl BackgroundTaskManager {
    pub async fn new(pool: Pool<Postgres>, config: MessengerConfig) -> Result<Self, IngesterError> {
        let messenger = RedisTaskMessenger::new(BG_TASK_STREAM_KEY, config).await?;
        Ok(BackgroundTaskManager {
            pool,
            producer: None,
            messenger,
        })
    }

    pub fn start(&mut self, config: Option<BgTaskConfig>) -> JoinHandle<()> {
        let (producer, _) = mpsc::unbounded_channel::<TaskData>();
        self.producer = Some(producer);

        let config = config.unwrap_or_default();

        let purge_time = tokio::time::Duration::from_secs(
            config
                .purge_time
                .unwrap_or(BgTaskConfig::default().purge_time.unwrap()),
        );
        let delete_interval = tokio::time::Duration::from_millis(
            config
                .delete_interval
                .unwrap_or(BgTaskConfig::default().delete_interval.unwrap()),
        );
        let queue_depth_interval = tokio::time::Duration::from_millis(
            config
                .query_depth_interval
                .unwrap_or(BgTaskConfig::default().query_depth_interval.unwrap()),
        );
        let reset_interval = tokio::time::Duration::from_secs(
            config
                .reset_interval_seconds
                .unwrap_or(BgTaskConfig::default().reset_interval_seconds.unwrap()),
        );
        let max_concurrent_manager_tasks = config.max_concurrent_manager_tasks.unwrap_or(
            BgTaskConfig::default()
                .max_concurrent_manager_tasks
                .unwrap(),
        );
        let batch_size = config
            .batch_size
            .unwrap_or(BgTaskConfig::default().batch_size.unwrap());

        info!(
            "Background task manager config: batch_size:{:?}, purge_time: {:?}, delete_interval: {:?}, queue_depth_interval: {:?}",
            batch_size, purge_time, delete_interval, queue_depth_interval
        );

        // Emit metrics for stream length periodically
        let messenger = self.messenger.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(queue_depth_interval);
            loop {
                interval.tick().await; // ticks immediately
                let res = BackgroundTaskManager::get_task_stream_len(&messenger).await;
                match res {
                    Ok(len) => {
                        debug!("Stream length for background tasks: {}", len);
                        metric! {
                            statsd_gauge!("ingester.bgtask.stream_len", len);
                        }
                    }
                    Err(e) => {
                        error!("error getting stream length in bg_task_manager: {}", e);
                    }
                };
            }
        });

        // Emit metrics for `Pending` task count in `tasks` table periodically
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
            let mut interval = time::interval(queue_depth_interval);
            loop {
                interval.tick().await; // ticks immediately
                let res = BackgroundTaskManager::get_pending_task_count(&conn).await;
                match res {
                    Ok(count) => {
                        debug!("Pending task count: {}", count);
                        metric! {
                            statsd_gauge!("ingester.bgtask.pending_count", count);
                        }
                    }
                    Err(e) => {
                        error!("error getting pending count in bg_task_manager: {}", e);
                    }
                };
            }
        });

        // Emit metrics for `Running` task count in `tasks` table periodically
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
            let mut interval = time::interval(queue_depth_interval);
            loop {
                interval.tick().await; // ticks immediately
                let res = BackgroundTaskManager::get_running_task_count(&conn).await;
                match res {
                    Ok(count) => {
                        debug!("Running task count: {}", count);
                        metric! {
                            statsd_gauge!("ingester.bgtask.running_count", count);
                        }
                    }
                    Err(e) => {
                        error!("error getting running count in bg_task_manager: {}", e);
                    }
                };
            }
        });

        // Purge old tasks periodically from `tasks` table
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
            let mut interval = time::interval(delete_interval);
            loop {
                interval.tick().await; // ticks immediately
                let delete_res = BackgroundTaskManager::purge_old_tasks(&conn, purge_time).await;
                match delete_res {
                    Ok(res) => {
                        info!("deleted {} tasks entries", res.rows_affected);
                        metric! {
                            statsd_count!("ingester.bgtask.purged_tasks", i64::try_from(res.rows_affected).unwrap_or(1));
                        }
                    }
                    Err(e) => {
                        metric! {
                            statsd_count!("ingester.bgtask.purge_error", 1);
                        }
                        error!("error deleting tasks: {}", e);
                    }
                };
            }
        });

        // Reset tasks stuck in `Running` state indefinitely.
        // This can happen if the task runner faces any error while/before marking the task as `Success`/`Failed`.
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
            let mut interval = time::interval(reset_interval);
            loop {
                interval.tick().await; // ticks immediately
                let res = BackgroundTaskManager::reset_stuck_tasks(&conn, reset_interval).await;
                match res {
                    Ok(res) => info!("reset {} tasks entries", res.rows_affected),
                    Err(e) => error!("error resetting tasks: {}", e),
                };
            }
        });

        // Poll DB for tasks and push them to Redis stream
        let pool = self.pool.clone();
        let messenger = self.messenger.clone();
        let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
        let stream = get_task_stream(conn, batch_size);
        tokio::spawn(async move {
            pin_mut!(stream);
            let res = stream
                .map(|result| async {
                    match result {
                        Ok(task) => {
                            let mut messenger = messenger.clone();
                            tokio::spawn(async move {
                                let task = Task {
                                    task_type: task.task_type,
                                    data: task.data,
                                    attempts: task.attempts,
                                };
                                let res = messenger.send_task(BG_TASK_STREAM_KEY, task).await;

                                if let Err(e) = res {
                                    metric! {
                                        statsd_count!("ingester.bgtask.stream_send_error", 1);
                                    }
                                    error!("Error while sending task to stream: {}", e);
                                }
                            });

                            Ok(())
                        }
                        Err(e) => Err(IngesterError::TaskManagerError(format!(
                            "Error getting tasks from db: {}",
                            e
                        ))),
                    }
                })
                .buffer_unordered(max_concurrent_manager_tasks)
                .try_collect::<()>()
                .await;

            if let Err(e) = res {
                error!("Error in background task manager: {}", e);
            }
        })
    }

    /// Get tasks from `tasks` table.
    async fn get_tasks(
        conn: &DatabaseConnection,
        batch_size: usize,
    ) -> Result<Vec<tasks::Model>, IngesterError> {
        tasks::Entity::find()
            .filter(
                Condition::all()
                    .add(
                        Condition::any()
                            .add(tasks::Column::Status.eq(TaskStatus::Pending)) // not sent to stream yet
                            .add(tasks::Column::Status.eq(TaskStatus::Failed)), // retryable
                    )
                    .add(
                        Condition::any()
                            .add(tasks::Column::LockedUntil.lte(Utc::now()))
                            .add(tasks::Column::LockedUntil.is_null()),
                    )
                    .add(
                        Expr::col(tasks::Column::Attempts)
                            .less_than(Expr::col(tasks::Column::MaxAttempts)),
                    ),
            )
            .order_by(tasks::Column::Attempts, Order::Asc)
            .order_by(tasks::Column::CreatedAt, Order::Desc)
            .limit(batch_size as u64)
            .all(conn)
            .await
            .map_err(|e| e.into())
    }

    /// Lock tasks in `tasks` table. To lock tasks, we mark them as `Running`.
    /// Also refreshes `created_at` so that `reset_stuck_tasks` measures time since
    /// the task started running, not when it was first created.
    async fn lock_tasks(
        conn: &DatabaseConnection,
        tasks: Vec<tasks::Model>,
    ) -> Result<UpdateResult, IngesterError> {
        let ids = tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>();
        tasks::Entity::update_many()
            .col_expr(
                tasks::Column::Status,
                Expr::value(TaskStatus::Running).cast_as(TaskStatusEnum),
            )
            .col_expr(
                tasks::Column::CreatedAt,
                Expr::value(chrono::Utc::now().naive_utc()),
            )
            .filter(Condition::all().add(tasks::Column::Id.is_in(ids)))
            .exec(conn)
            .await
            .map_err(|e| e.into())
    }

    /// Purges old tasks from the `tasks` table.
    async fn purge_old_tasks(
        conn: &DatabaseConnection,
        task_max_age: time::Duration,
    ) -> Result<DeleteResult, IngesterError> {
        let interval = format!(
            "NOW() - created_at::timestamp > interval '{} seconds'",
            task_max_age.as_secs()
        );
        let cond = Expr::cust(&interval);
        let status_cond = Condition::any()
            .add(tasks::Column::Status.eq(TaskStatus::Success))
            .add(tasks::Column::Status.eq(TaskStatus::Failed));
        tasks::Entity::delete_many()
            .filter(Condition::all().add(cond).add(status_cond))
            .exec(conn)
            .await
            .map_err(|e| e.into())
    }

    /// Make `Running` tasks `Pending` that have been stuck in that state for more than `duration` seconds.
    async fn reset_stuck_tasks(
        conn: &DatabaseConnection,
        duration: time::Duration,
    ) -> Result<UpdateResult, IngesterError> {
        let interval = format!(
            "created_at::timestamp <= NOW() - interval '{} seconds'",
            duration.as_secs()
        );
        let cond = Expr::cust(&interval);
        tasks::Entity::update_many()
            .col_expr(
                tasks::Column::Status,
                Expr::value(TaskStatus::Pending).cast_as(TaskStatusEnum),
            )
            .filter(
                Condition::all()
                    .add(cond)
                    .add(tasks::Column::Status.eq(TaskStatus::Running)),
            )
            .exec(conn)
            .await
            .map_err(|e| e.into())
    }

    /// Get count of `Pending` tasks in `tasks` table.
    async fn get_pending_task_count(conn: &DatabaseConnection) -> Result<u64, IngesterError> {
        tasks::Entity::find()
            .filter(tasks::Column::Status.eq(TaskStatus::Pending))
            .count(conn)
            .await
            .map_err(|e| e.into())
    }

    /// Get count of `Running` tasks in `tasks` table.
    async fn get_running_task_count(conn: &DatabaseConnection) -> Result<u64, IngesterError> {
        tasks::Entity::find()
            .filter(tasks::Column::Status.eq(TaskStatus::Running))
            .count(conn)
            .await
            .map_err(|e| e.into())
    }

    /// Get Redis stream size.
    async fn get_task_stream_len(messenger: &RedisTaskMessenger) -> Result<u64, IngesterError> {
        let mut messenger = messenger.clone();
        messenger.get_stream_size(BG_TASK_STREAM_KEY).await
    }
}

fn get_task_stream(
    conn: DatabaseConnection,
    batch_size: usize,
) -> impl Stream<Item = Result<tasks::Model, IngesterError>> {
    let stream = stream! {
        loop {
            let res = BackgroundTaskManager::get_tasks(&conn, batch_size).await;
            match res {
                Ok(tasks) => {
                    let res = BackgroundTaskManager::lock_tasks(&conn, tasks.clone()).await;
                    if let Err(e) = res {
                        metric! {
                            statsd_count!("ingester.bgtask.lock_error", 1);
                        }
                        error!("error locking tasks: {}", e);
                        continue;
                    }

                    for t in tasks {
                        yield Ok(t);
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
