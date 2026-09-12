use std::sync::Arc;

use crate::{
    config::IngesterConfig, metric, metrics::capture_result,
    program_transformers::ProgramTransformer, tasks::TaskData,
};
use cadence_macros::{is_global_default_set, statsd_count, statsd_time};
use chrono::Utc;
use log::{debug, error, warn};
use plerkle_messenger::{ConsumptionType, Messenger, RecvData, TRANSACTION_STREAM};
use plerkle_serialization::root_as_transaction_info;
use plerkle_serialization::TransactionInfo;
use solana_sdk::pubkey::Pubkey;

use sqlx::{Pool, Postgres};
use tokio::{
    sync::mpsc::UnboundedSender,
    task::{JoinHandle, JoinSet},
    time::Instant,
};

fn is_helium_transaction(tx: &TransactionInfo) -> bool {
    if let Some(account_keys) = tx.account_keys() {
        for key in account_keys {
            if let Ok(pubkey) = Pubkey::try_from(key.0.as_slice()) {
                if pubkey.to_string() == "memMa1HG4odAFmUbGWfPwS1WWfK95k99F2YTkGvyxZr" {
                    return true;
                }
            }
        }
    }
    false
}

pub fn transaction_worker<T: Messenger>(
    pool: Pool<Postgres>,
    config: IngesterConfig,
    bg_task_sender: UnboundedSender<TaskData>,
    ack_channel: UnboundedSender<(&'static str, String)>,
    consumption_type: ConsumptionType,
) -> JoinHandle<()> {
    let stream_key = TRANSACTION_STREAM; // TODO: send txns to TXN_BACKFILL stream on full flush (from plugin)
    tokio::spawn(async move {
        let source = T::new(config.get_messenger_client_config()).await;
        if let Ok(mut msg) = source {
            let manager = Arc::new(ProgramTransformer::new(pool, bg_task_sender, config));
            loop {
                let e = msg.recv(stream_key, consumption_type.clone()).await;
                let mut tasks = JoinSet::new();
                match e {
                    Ok(data) => {
                        let len = data.len();
                        for item in data {
                            tasks.spawn(handle_transaction(Arc::clone(&manager), item, stream_key));
                        }
                        if len > 0 {
                            debug!("Processed {} txns", len);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving from txn stream: {}", e);
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
                                    error!("Txn stream ack error: {}", err);
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

async fn handle_transaction(
    manager: Arc<ProgramTransformer>,
    item: RecvData,
    stream_key: &str,
) -> Option<String> {
    let mut ret_id = None;
    if item.tries > 0 {
        metric! {
            statsd_count!("ingester.stream_redelivery", 1, "stream" => stream_key);
        }
    }
    let id = item.id.to_string();
    let tx_data = item.data;
    match root_as_transaction_info(&tx_data) {
        Ok(tx) => {
            let signature = tx.signature().unwrap_or("NO SIG");

            let is_helium = is_helium_transaction(&tx);
            if is_helium {
                warn!("HELIUM memMa1HG4: Processing transaction: {} (stream: {}, tries: {}, data_len: {})",
                    signature, stream_key, item.tries, tx_data.len());
            }
            debug!("Received transaction: {}", signature);
            metric! {
                statsd_count!("ingester.seen", 1, "stream" => stream_key);
            }
            let seen_at = Utc::now();
            metric! {
                statsd_time!(
                    "ingester.bus_ingest_time",
                    std::cmp::min(seen_at.timestamp_millis() - tx.seen_at(), 0) as u64,
                    "stream" => stream_key
                );
            }

            let begin = Instant::now();
            let res = manager.handle_transaction(&tx).await;
            if is_helium {
                match &res {
                    Ok(_) => warn!(
                        "HELIUM memMa1HG4: Successfully handled transaction: {}",
                        signature
                    ),
                    Err(e) => warn!(
                        "HELIUM memMa1HG4: Failed to handle transaction {}: {:?}",
                        signature, e
                    ),
                }
            }
            let should_ack = capture_result(
                stream_key,
                ("txn", "txn"),
                item.tries,
                res,
                begin,
                tx.signature(),
                None,
            );
            if should_ack {
                ret_id = Some(id);
            }
        }
        Err(e) => {
            error!(
                "Failed to deserialize transaction from stream '{}': {:?}",
                stream_key, e
            );
            error!("Failed tx_data preview: {:?}", tx_data);
        }
    }
    ret_id
}
