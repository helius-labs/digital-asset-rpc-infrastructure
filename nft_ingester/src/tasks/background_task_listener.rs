use super::{BgTask, TaskData};
use crate::tasks::is_global_default_set;
use crate::{error::IngesterError, metric, tasks::hash_task};
use cadence_macros::{statsd_count, statsd_histogram};
use chrono::Utc;
use digital_asset_types::dao::{sea_orm_active_enums::TaskStatus, tasks};
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, QueryFilter, QueryTrait, Set,
    SqlxPostgresConnector,
};
use sqlx::{Pool, Postgres};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::mpsc::{self, UnboundedSender},
    task::JoinHandle,
};

/// `BackgroundTaskListener` listens for new tasks and saves them to the DB.
/// It also provides a unbouned sender to send tasks to the `BackgroundTaskListener`.
pub struct BackgroundTaskListener {
    pool: Pool<Postgres>,
    producer: Option<UnboundedSender<TaskData>>,
    registered_task_types: Arc<HashMap<String, Box<dyn BgTask>>>,
}

impl BackgroundTaskListener {
    pub async fn new(
        pool: Pool<Postgres>,
        registered_task_types: Arc<HashMap<String, Box<dyn BgTask>>>,
    ) -> Result<Self, IngesterError> {
        Ok(BackgroundTaskListener {
            pool,
            producer: None,
            registered_task_types,
        })
    }

    /// Get an unbounded sender to send tasks to the `BackgroundTaskListener`.
    pub fn get_sender(&self) -> Result<UnboundedSender<TaskData>, IngesterError> {
        self.producer
            .clone()
            .ok_or(IngesterError::TaskListenerNotStarted)
    }

    /// Start a `BackgroundTaskListener` which listens for new tasks and saves them to the DB.
    pub fn start(&mut self) -> JoinHandle<()> {
        let (producer, mut receiver) = mpsc::unbounded_channel::<TaskData>();
        self.producer = Some(producer);

        // Listens for new tasks and saves them to the `tasks` table.
        let pool = self.pool.clone();
        let task_map = self.registered_task_types.clone();
        tokio::task::spawn(async move {
            while let Some(task) = receiver.recv().await {
                if let Some(task_created_time) = task.created_at {
                    #[allow(deprecated)]
                    let bus_time =
                        Utc::now().timestamp_millis() - task_created_time.timestamp_millis();
                    metric! {
                        statsd_histogram!("ingester.bgtask.bus_time", bus_time as u64, "type" => task.name);
                    }
                }

                if let Ok(hash) = hash_task(task.name.to_string(), task.data.clone()) {
                    let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
                    let task_entry = tasks::Entity::find_by_id(hash.clone())
                        .filter(tasks::Column::Status.ne(TaskStatus::Pending))
                        .one(&conn)
                        .await;
                    if let Ok(Some(e)) = task_entry {
                        metric! {
                            statsd_count!("ingester.bgtask.identical", 1, "type" => &e.task_type);
                        }
                        continue;
                    }
                    metric! {
                        statsd_count!("ingester.bgtask.new", 1, "type" => &task.name);
                    }
                    BackgroundTaskListener::save_new_task(pool.clone(), task, task_map.clone());
                }
            }
        })
    }

    pub fn save_new_task(
        pool: Pool<Postgres>,
        task: TaskData,
        tasks_def: Arc<HashMap<String, Box<dyn BgTask>>>,
    ) -> JoinHandle<Result<(), IngesterError>> {
        let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
        tokio::task::spawn(async move {
            if let Some(task_executor) = tasks_def.get(task.name) {
                let model = tasks::ActiveModel {
                    id: Set(hash_task(task.name.to_string(), task.data.clone())?),
                    task_type: Set(task.name.to_string()),
                    data: Set(task.data.clone()),
                    status: Set(TaskStatus::Pending),
                    created_at: Set(Utc::now().naive_utc()),
                    locked_until: Set(None),
                    locked_by: Set(None),
                    max_attempts: Set(task_executor.max_attempts()),
                    attempts: Set(0),
                    duration: Set(None),
                    errors: Set(None),
                };
                let query = tasks::Entity::insert(model)
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
                conn.execute(query).await.map(|_| ()).map_err(|e| e.into())
            } else {
                Err(IngesterError::TaskManagerError(format!(
                    "{} not a valid task type",
                    task.name
                )))
            }
        })
    }
}
