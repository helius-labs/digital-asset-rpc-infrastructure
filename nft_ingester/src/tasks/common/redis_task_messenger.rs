use crate::{error::IngesterError, tasks::Task};
use log::{debug, error, info, warn};
use plerkle_messenger::MessengerConfig;
use redis::{
    aio::ConnectionManager,
    cmd,
    streams::{
        StreamId, StreamKey, StreamMaxlen, StreamPendingCountReply, StreamRangeReply,
        StreamReadOptions, StreamReadReply,
    },
    AsyncCommands, RedisResult, Value,
};
use std::collections::HashMap;

const REDIS_CON_STR: &str = "redis_connection_str";
const GROUP_NAME: &str = "offchain-task-messenger";
const MAX_RETRIES: usize = 3;
const MIN_IDLE_TIME_MS: usize = 5_000;
const STREAM_MAX_LEN: usize = 10_000_000;

pub struct StreamTask {
    pub task: Task,
    pub stream_id: String,
}

#[derive(Clone)]
pub struct RedisTaskMessenger {
    connection: ConnectionManager,
    consumer_group_name: String,
    consumer_id: String,
    max_retries: usize,
    min_idle_time_ms: usize,
}

impl RedisTaskMessenger {
    pub async fn new(stream_key: &str, config: MessengerConfig) -> Result<Self, IngesterError> {
        // Setup Redis client.
        let uri = config
            .get(REDIS_CON_STR)
            .and_then(|u| u.clone().into_string())
            .ok_or(IngesterError::TaskManagerMessengerError {
                msg: format!("Connection String Missing: {}", REDIS_CON_STR),
            })?;
        let client = redis::Client::open(uri).unwrap();

        // Get connection.
        let connection = client.get_tokio_connection_manager().await.map_err(|e| {
            error!("{}", e.to_string());
            IngesterError::TaskManagerMessengerError { msg: e.to_string() }
        })?;

        let max_retries = config
            .get("retries")
            .and_then(|r| r.clone().to_u128().map(|n| n as usize))
            .unwrap_or(MAX_RETRIES);

        let min_idle_time_ms = config
            .get("idle_timeout")
            .and_then(|r| r.clone().to_u128().map(|n| n as usize))
            .unwrap_or(MIN_IDLE_TIME_MS);

        let consumer_id = config
            .get("consumer_id")
            .and_then(|id| id.clone().into_string())
            // Using the previous default name when the configuration does not
            // specify any particular consumer_id.
            .unwrap_or(String::from("task_manager"));

        let consumer_group_name = config
            .get("consumer_group_name")
            .and_then(|r| r.clone().into_string())
            .unwrap_or(GROUP_NAME.to_string());

        // add stream to Redis
        let result: RedisResult<()> = connection
            .clone()
            .xgroup_create_mkstream(stream_key, consumer_group_name.as_str(), "$")
            .await;
        if let Err(e) = result {
            info!("Group already exists: {:?}", e)
        }

        Ok(Self {
            connection,
            consumer_group_name,
            consumer_id,
            max_retries,
            min_idle_time_ms,
        })
    }

    /// Get Redis stream size.
    pub async fn get_stream_size(
        &mut self,
        stream_key: &'static str,
    ) -> Result<u64, IngesterError> {
        let result: RedisResult<u64> = self.connection.xlen(stream_key).await;
        match result {
            Ok(reply) => Ok(reply),
            Err(e) => Err(IngesterError::TaskManagerMessengerError { msg: e.to_string() }),
        }
    }

    /// Send serialized task data to Redis stream.
    pub async fn send_task(
        &mut self,
        stream_key: &'static str,
        task: Task,
    ) -> Result<(), IngesterError> {
        let task_bytes = self.serialize_task(task)?;
        let maxlen = StreamMaxlen::Approx(STREAM_MAX_LEN);

        // Put serialized task data into Redis.
        let result: RedisResult<String> = self
            .connection
            .xadd_maxlen(stream_key, maxlen, "*", &[("data", &task_bytes)])
            .await;
        result.map_err(|e| {
            error!("Could not send task data to Redis stream: {e}");
            IngesterError::TaskManagerMessengerError { msg: e.to_string() }
        })?;

        debug!("Data Sent to {}", stream_key);
        Ok(())
    }

    /// Receive tasks from Redis stream.
    pub async fn recv_tasks(
        &mut self,
        stream_key: &'static str,
        batch_size: usize,
    ) -> Result<Vec<StreamTask>, IngesterError> {
        let mut stream_tasks: Vec<StreamTask> = Vec::with_capacity(batch_size * 2);

        let opts = StreamReadOptions::default()
            .count(batch_size)
            .group(self.consumer_group_name.as_str(), self.consumer_id.as_str());

        let reply: StreamReadReply = self
            .connection
            .xread_options(&[stream_key], &[">"], &opts)
            .await
            .map_err(|e| {
                error!("Redis receive error: {e}");
                IngesterError::TaskManagerMessengerError { msg: e.to_string() }
            })?;

        // Parse data in stream read reply and store in Vec to return to caller.
        for StreamKey { key: _, ids } in reply.keys.into_iter() {
            for StreamId { id, map } in ids {
                let data = if let Some(data) = map.get("data") {
                    data
                } else {
                    error!("No Data was stored in Redis for ID {id}");
                    continue;
                };
                let bytes = match data {
                    Value::Data(bytes) => bytes,
                    _ => {
                        error!("Redis data for ID {id} in wrong format");
                        continue;
                    }
                };

                // deserialize the task and push it to the vector
                let task = self.deserialize_task(bytes);
                if let Ok(task) = task {
                    let stream_task = StreamTask {
                        task,
                        stream_id: id,
                    };
                    stream_tasks.push(stream_task);
                } else {
                    error!("Could not deserialize task. StreamID: {id}");
                }
            }
        }

        // Get redelivered tasks
        let xauto_reply = self.xautoclaim(stream_key, batch_size).await;
        match xauto_reply {
            Ok(reply) => {
                let mut pending_tasks = reply;
                stream_tasks.append(&mut pending_tasks);
            }
            Err(e) => {
                error!("XPENDING ERROR {e}");
            }
        }

        Ok(stream_tasks)
    }

    pub async fn ack(
        &mut self,
        stream_key: &'static str,
        ids: &[String],
    ) -> Result<(), IngesterError> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut pipe = redis::pipe();
        pipe.xack(stream_key, self.consumer_group_name.as_str(), ids);
        pipe.xdel(stream_key, ids);

        pipe.query_async(&mut self.connection)
            .await
            .map_err(|e| IngesterError::TaskManagerMessengerError { msg: e.to_string() })
    }

    async fn xautoclaim(
        &mut self,
        stream_key: &'static str,
        batch_size: usize,
    ) -> Result<Vec<StreamTask>, IngesterError> {
        let mut xauto = cmd("XAUTOCLAIM");
        xauto
            .arg(stream_key)
            .arg(self.consumer_group_name.clone())
            .arg(self.consumer_id.as_str())
            // Reclaim tasks pending for more than `min_idle_time_ms` milliseconds
            .arg(self.min_idle_time_ms)
            // Reclaim messages having an equal or greater ID than "0-0"
            .arg("0-0")
            .arg("COUNT")
            .arg(batch_size);

        let result: (String, StreamRangeReply, Vec<String>) = xauto
            .query_async(&mut self.connection)
            .await
            .map_err(|e| IngesterError::TaskManagerMessengerError { msg: e.to_string() })?;

        let range_reply = result.1;
        if range_reply.ids.is_empty() {
            // We've reached the end of the PEL.
            return Ok(Vec::new());
        }

        let mut stream_tasks: Vec<StreamTask> = Vec::new();
        let first = range_reply.ids.first().unwrap();
        let last = range_reply.ids.last().unwrap();

        // We need to use `xpending_count` to get a `StreamPendingCountReply` which contains
        // information about the number of times a message has been delivered.
        let pending_result: StreamPendingCountReply = self
            .connection
            .xpending_count(
                stream_key,
                self.consumer_group_name.clone(),
                &first.id.clone(),
                &last.id.clone(),
                range_reply.ids.len(),
            )
            .await
            .map_err(|e| {
                error!("Redis receive error: {e}");
                IngesterError::TaskManagerMessengerError { msg: e.to_string() }
            })?;
        let mut pending = HashMap::new();
        let mut ack_list = Vec::new();
        let prs = pending_result.ids.into_iter();
        for pr in prs {
            pending.insert(pr.id.clone(), pr);
        }
        for sid in range_reply.ids {
            let StreamId { id, map } = sid;
            let info = if let Some(info) = pending.get(&id) {
                info
            } else {
                warn!("No pending info for ID {id}");
                continue;
            };
            let data = if let Some(data) = map.get("data") {
                data
            } else {
                info!("No Data was stored in Redis for ID {id}");
                continue;
            };

            let bytes = match data {
                Value::Data(bytes) => bytes,
                _ => {
                    error!("Redis data for Stream ID {id} in wrong format");
                    continue;
                }
            };

            if info.times_delivered > self.max_retries {
                debug!(
                    "Message has reached {} retries. StreamId: {id}",
                    info.times_delivered
                );
                ack_list.push(id.clone());
                continue;
            }

            let task = self.deserialize_task(bytes);
            if let Ok(task) = task {
                let stream_task = StreamTask {
                    task,
                    stream_id: id,
                };
                stream_tasks.push(stream_task);
            } else {
                error!("Could not deserialize task. StreamID: {id}");
            }
        }
        if let Err(e) = self.ack(stream_key, &ack_list).await {
            error!("Error acking pending messages: {}", e);
        }
        Ok(stream_tasks)
    }

    fn serialize_task(&self, task: Task) -> Result<Vec<u8>, IngesterError> {
        let task_str = serde_json::to_string(&task);
        match task_str {
            Ok(task_str) => {
                let task_bytes = task_str.as_bytes();
                Ok(task_bytes.to_owned())
            }
            Err(e) => Err(IngesterError::TaskManagerMessengerError { msg: e.to_string() }),
        }
    }

    fn deserialize_task(&self, bytes: &Vec<u8>) -> Result<Task, IngesterError> {
        let task_str = std::str::from_utf8(bytes);
        match task_str {
            Ok(task_str) => {
                let task: Task = serde_json::from_str(&task_str)?;
                Ok(task)
            }
            Err(e) => Err(IngesterError::TaskManagerMessengerError { msg: e.to_string() }),
        }
    }
}
