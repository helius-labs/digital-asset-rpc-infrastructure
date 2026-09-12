use std::sync::Arc;

use crate::{
    config::{IngesterConfig, PodType},
    metric,
    metrics::capture_result,
    program_transformers::ProgramTransformer,
    tasks::TaskData,
};
use cadence_macros::{is_global_default_set, statsd_count, statsd_time};
use chrono::Utc;
use log::{debug, error};
use plerkle_messenger::{ConsumptionType, Messenger, RecvData, ACCOUNT_STREAM, ACC_BACKFILL};
use plerkle_serialization::root_as_account_info;
use sqlx::{Pool, Postgres};
use tokio::{
    sync::mpsc::UnboundedSender,
    task::{JoinHandle, JoinSet},
    time::Instant,
};

pub fn account_worker<T: Messenger>(
    pool: Pool<Postgres>,
    config: IngesterConfig,
    bg_task_sender: UnboundedSender<TaskData>,
    ack_channel: UnboundedSender<(&'static str, String)>,
    consumption_type: ConsumptionType,
    pod_type: &PodType,
) -> JoinHandle<()> {
    let stream_key = match pod_type {
        PodType::Backfiller => ACC_BACKFILL,
        PodType::Regular => ACCOUNT_STREAM,
    };

    tokio::spawn(async move {
        let config_clone = config.clone();
        let source = T::new(config_clone.get_messenger_client_config()).await;
        if let Ok(mut msg) = source {
            let manager = Arc::new(ProgramTransformer::new(pool, bg_task_sender, config));
            loop {
                let e: Result<Vec<RecvData>, plerkle_messenger::MessengerError> =
                    msg.recv(stream_key, consumption_type.clone()).await;
                let mut tasks = JoinSet::new();
                match e {
                    Ok(data) => {
                        let len = data.len();
                        for item in data {
                            tasks.spawn(handle_account(
                                Arc::clone(&manager),
                                item,
                                stream_key,
                                config_clone.clone(),
                            ));
                        }
                        if len > 0 {
                            debug!("Processed {} accounts", len);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving from account stream: {}", e);
                        metric! {
                            statsd_count!("ingester.stream.receive_error", 1, "stream" => stream_key);
                        }
                    }
                }
                while let Some(res) = tasks.join_next().await {
                    if let Ok(id) = res {
                        if let Some(id) = id {
                            let send = ack_channel.send((stream_key, id));
                            if let Err(err) = send {
                                metric! {
                                    error!("Account stream ack error: {}", err);
                                    statsd_count!("ingester.stream.ack_error", 1, "stream" => stream_key);
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

async fn handle_account(
    manager: Arc<ProgramTransformer>,
    item: RecvData,
    stream_key: &str,
    config: IngesterConfig,
) -> Option<String> {
    let id = item.id;
    let mut ret_id = None;
    let data = item.data;
    if item.tries > 0 {
        metric! {
            statsd_count!("ingester.account_stream_redelivery", 1);
        }
    }
    // Get root of account flatbuffers object.
    if let Ok(account_update) = root_as_account_info(&data) {
        let str_program_id =
            bs58::encode(account_update.owner().unwrap().0.as_slice()).into_string();

        metric! {
            statsd_count!("ingester.seen", 1, "owner" => &str_program_id, "stream" => stream_key);
            let seen_at = Utc::now();
            statsd_time!(
                "ingester.bus_ingest_time",
                std::cmp::max(seen_at.timestamp_millis() - account_update.seen_at(), 0) as u64,
                "owner" => &str_program_id,
                "stream" => stream_key
            );
        }
        let mut account = None;
        if let Some(pubkey) = account_update.pubkey() {
            account = Some(bs58::encode(pubkey.0.as_slice()).into_string());
        }
        let begin_processing = Instant::now();
        let res = manager.handle_account_update(account_update, &config).await;
        let should_ack = capture_result(
            stream_key,
            ("owner", &str_program_id),
            item.tries,
            res,
            begin_processing,
            None,
            account,
        );
        if should_ack {
            ret_id = Some(id);
        }
    }
    ret_id
}
