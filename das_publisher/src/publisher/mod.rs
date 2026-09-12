use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::fetchers::poller::is_bubblegum_transaction;
use crate::monitor::fetch_current_slot_with_infinite_retry;
use cadence_macros::{statsd_count, statsd_time};
use common::metric;
use digital_asset_types::dao::blocks;
use futures::future::join_all;
use futures::StreamExt;
use futures::{pin_mut, Stream};
use log::{error, info};
use plerkle_messenger::{
    select_messenger, Messenger, MessengerConfig, MessengerError, ACCOUNT_STREAM, ACC_BACKFILL,
    BLOCK_STREAM, SLOT_STREAM, TRANSACTION_STREAM, TXN_BACKFILL,
};
use plerkle_serialization::serializer::{
    seralize_encoded_transaction_with_status, serialize_account,
};
use plerkle_serialization::solana_geyser_plugin_interface_shims::ReplicaAccountInfoV2;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ConnectionTrait, QueryTrait, Set};
use sea_orm::{DatabaseConnection, EntityTrait};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta, EncodedTransactionWithStatusMeta,
};
use tokio::time::sleep;

use crate::fetchers::utils::{BlockMetadata, DASBlock};

const LOGGING_FREQUENCY: u64 = 100;
const ACCOUNT_STREAM_SIZE: usize = 600_000_000;
const TRANSACTION_STREAM_SIZE: usize = 100_000_000;
const BLOCK_STREAM_SIZE: usize = 100_000;
const STREAM_SIZE_THRESHOLD_BEFORE_SLEEP: u64 = 100_000;

pub async fn publish_block_stream(
    block_stream: impl Stream<Item = DASBlock>,
    db: DatabaseConnection,
    mut messenger_pool: Vec<Box<dyn Messenger>>,
    rpc_client: Arc<RpcClient>,
    last_indexed_slot_at_start: u64,
) {
    pin_mut!(block_stream);
    let current_slot = fetch_current_slot_with_infinite_retry(&rpc_client).await;
    let number_of_blocks_to_backfill = if current_slot > last_indexed_slot_at_start {
        current_slot - last_indexed_slot_at_start
    } else {
        0
    };
    info!(
        "Backfilling historical blocks. Current number of blocks to backfill: {}",
        number_of_blocks_to_backfill
    );
    let mut last_indexed_slot = last_indexed_slot_at_start;
    let mut finished_backfill_slot = None;
    let db = Arc::new(db);

    while let Some(block) = block_stream.next().await {
        let last_slot_in_block = block.block_metadata.slot;
        let block_utc_time = block.block_metadata.block_time;
        let block_time = UNIX_EPOCH + Duration::from_secs(block_utc_time as u64);
        let now = SystemTime::now();

        publish_block_with_infinite_retries(db.clone(), &mut messenger_pool, block).await;
        let latency = now.duration_since(block_time).unwrap_or_default();

        metric! {
            statsd_time!("block_publish_latency", latency.as_millis() as u64);
        }

        for slot in (last_indexed_slot + 1)..(last_slot_in_block + 1) {
            let blocks_indexed = slot - last_indexed_slot_at_start;
            if blocks_indexed < number_of_blocks_to_backfill {
                if blocks_indexed % LOGGING_FREQUENCY == 0 {
                    info!(
                        "Backfilled {} / {} blocks",
                        blocks_indexed, number_of_blocks_to_backfill
                    );
                }
            } else {
                if finished_backfill_slot.is_none() {
                    info!("Finished backfilling historical blocks!");
                    info!("Starting to index new blocks...");
                    finished_backfill_slot = Some(slot);
                }
                if slot % LOGGING_FREQUENCY == 0 {
                    info!("Indexed slot {}", slot);
                }
            }
            last_indexed_slot = slot;
        }
        let first_messenger = messenger_pool.first_mut().unwrap();
        loop {
            let acc_stream_size =
                fetch_stream_size_with_infinite_retries(first_messenger, ACCOUNT_STREAM).await;
            if acc_stream_size > STREAM_SIZE_THRESHOLD_BEFORE_SLEEP as u64 {
                info!(
                    "Sleeping because account stream size is greater than {}",
                    STREAM_SIZE_THRESHOLD_BEFORE_SLEEP
                );
                sleep(Duration::from_secs(1)).await;
            } else {
                break;
            }
        }

        loop {
            let txn_stream_size =
                fetch_stream_size_with_infinite_retries(first_messenger, TRANSACTION_STREAM).await;
            if txn_stream_size > STREAM_SIZE_THRESHOLD_BEFORE_SLEEP as u64 {
                info!(
                    "Sleeping because transaction stream size is greater than {}",
                    STREAM_SIZE_THRESHOLD_BEFORE_SLEEP
                );
                sleep(Duration::from_secs(1)).await;
            } else {
                break;
            }
        }
    }
}

pub async fn fetch_stream_size_with_infinite_retries(
    messenger: &mut Box<dyn Messenger>,
    stream_key: &'static str,
) -> u64 {
    loop {
        let res = messenger.stream_size(stream_key).await;
        match res {
            Ok(size) => return size,
            Err(e) => {
                error!("Error fetching stream size: {:?}", e);
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn publish_block_with_infinite_retries(
    db: Arc<DatabaseConnection>,
    messenger_pool: &mut Vec<Box<dyn Messenger>>,
    block: DASBlock,
) {
    let block_utc_time = block.block_metadata.block_time;
    publish_block_accounts_updates_with_infinite_retries(messenger_pool, block.das_accounts).await;
    publish_block_transactions_with_infinite_retries(
        messenger_pool,
        block.das_transactions,
        block.block_metadata.slot,
        block.block_metadata.block_time,
    )
    .await;
    store_block_metadata_with_infinite_retries(db.clone(), block.block_metadata).await;

    let block_time = UNIX_EPOCH + Duration::from_secs(block_utc_time as u64);
    let now = SystemTime::now();
    let latency = now.duration_since(block_time).unwrap_or_default();
    metric! {
        statsd_time!(
            "block_publish_latency",
            latency.as_millis() as u64
        );
    }
}

async fn store_block_metadata_with_infinite_retries(
    db: Arc<DatabaseConnection>,
    block_metadata: BlockMetadata,
) {
    loop {
        let block_model = blocks::ActiveModel {
            slot: Set(block_metadata.slot as i64),
            parent_slot: Set(block_metadata.parent_slot as i64),
            block_time: Set(block_metadata.block_time),
            blockhash: Set(block_metadata.blockhash.clone().into()),
            parent_blockhash: Set(block_metadata.parent_blockhash.clone().into()),
            block_height: Set(block_metadata.block_height as i64),
        };
        // We first build the query and then execute it because SeaORM has a bug where it always throws
        // expected not to insert anything if the key already exists.
        let query = blocks::Entity::insert(block_model)
            .on_conflict(
                OnConflict::column(blocks::Column::Slot)
                    .do_nothing()
                    .to_owned(),
            )
            .build(db.as_ref().get_database_backend());
        let res = db.as_ref().execute(query).await;
        match res {
            Ok(_) => break,
            Err(e) => {
                error!("Error inserting block metadata: {:?}", e);
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
async fn publish_block_accounts_updates_with_infinite_retries(
    messenger_pool: &mut Vec<Box<dyn Messenger>>,
    accounts: HashMap<Pubkey, (Option<Account>, u64)>,
) {
    let start = Instant::now();
    let num_accounts = accounts.len();
    let mut account_chunks = Vec::with_capacity(messenger_pool.len());
    for _ in 0..messenger_pool.len() {
        account_chunks.push(Vec::new());
    }
    for (i, (pubkey, (account, slot))) in accounts.into_iter().enumerate() {
        let index = i % messenger_pool.len();
        account_chunks[index].push((pubkey, (account, slot)));
    }
    let account_update_futures = account_chunks
        .into_iter()
        .zip(messenger_pool.iter_mut())
        .map(|(chunk, messenger)| {
            let chunk = chunk.clone();
            let chunk_start = Instant::now();
            async move {
                for (pubkey, (account, slot)) in chunk {
                    let account_bytes = encode_account_update(&pubkey, account, slot);
                    let send_start = Instant::now();
                    loop {
                        let res = messenger.send(ACCOUNT_STREAM, &account_bytes).await;
                        match res {
                            Ok(_) => {
                                metric! {
                                    statsd_count!("account_send_success", 1);
                                    statsd_time!(
                                        "account_send_latency",
                                        send_start.elapsed().as_millis() as u64
                                    );
                                }
                                break;
                            }
                            Err(e) => {
                                metric! {
                                    statsd_count!("account_send_error", 1);
                                }
                                error!("Error sending account update: {:?}", e);
                                sleep(Duration::from_secs(1)).await;
                            }
                        }
                    }
                }
                loop {
                    let res = messenger.flush(ACCOUNT_STREAM).await;
                    match res {
                        Ok(_) => break,
                        Err(e) => {
                            error!("Error flushing account updates: {:?}", e);
                            sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
                metric! {
                    statsd_time!(
                        "account_chunk_processing_time",
                        chunk_start.elapsed().as_millis() as u64
                    );
                }
            }
        });
    join_all(account_update_futures).await;

    metric! {
        statsd_count!("accounts_processed", num_accounts as i64);
    }
    metric! {
        statsd_time!(
            "accounts_batch_processing_time",
            start.elapsed().as_millis() as u64
        );
    }
}

async fn publish_block_transactions_with_infinite_retries(
    messenger_pool: &mut Vec<Box<dyn Messenger>>,
    transactions: Vec<EncodedTransactionWithStatusMeta>,
    slot: u64,
    block_time: i64,
) {
    let start = Instant::now();
    let num_transactions = transactions.len();
    let mut transaction_chunks = Vec::new();
    for _ in 0..messenger_pool.len() {
        transaction_chunks.push(Vec::new());
    }
    for (i, transaction) in transactions.into_iter().enumerate() {
        let index = i % messenger_pool.len();
        transaction_chunks[index].push(transaction);
    }
    let transaction_futures = transaction_chunks
        .into_iter()
        .zip(messenger_pool.iter_mut())
        .map(|(chunk, messenger)| {
            let chunk = chunk.clone();
            let chunk_start = Instant::now();
            async move {
                for transaction in chunk {
                    let send_start = Instant::now();

                    let keys = crate::fetchers::poller::get_transaction_keys(transaction.clone());
                    let is_helium = keys.iter().any(|k| k.to_string() == "memMa1HG4odAFmUbGWfPwS1WWfK95k99F2YTkGvyxZr");

                    let tx_sig = if is_helium {
                        transaction.transaction.decode()
                            .and_then(|t| t.signatures.first().map(|s| s.to_string()))
                            .unwrap_or_else(|| "UNKNOWN".to_string())
                    } else {
                        String::new()
                    };

                    if !is_bubblegum_transaction(transaction.clone()) {
                        if is_helium {
                            error!("HELIUM memMa1HG4: Skipping Helium transaction {} - no Bubblegum program found!", tx_sig);
                        }
                        continue;
                    }

                    if is_helium {
                        info!("HELIUM memMa1HG4: Publishing transaction {} to stream (slot: {})", tx_sig, slot);
                    }
                    let encoded_confirmed_transaction_with_status_meta =
                        EncodedConfirmedTransactionWithStatusMeta {
                            slot,
                            transaction: transaction.clone(),
                            block_time: Some(block_time),
                            transaction_index: None,
                        };

                    let tx_bytes =
                        encode_transaction_update(encoded_confirmed_transaction_with_status_meta);
                    loop {
                        let res = messenger.send(TRANSACTION_STREAM, &tx_bytes).await;
                        match res {
                            Ok(_) => {
                                metric! {
                                    statsd_count!("transaction_send_success", 1);
                                    statsd_time!(
                                        "transaction_send_latency",
                                        send_start.elapsed().as_millis() as u64
                                    );
                                }
                                break;
                            }
                            Err(e) => {
                                metric! {
                                    statsd_count!("transaction_send_error", 1);
                                }
                                error!("Error sending transaction update: {:?}", e);
                                sleep(Duration::from_secs(1)).await;
                            }
                        }
                    }
                }
                loop {
                    let res = messenger.flush(TRANSACTION_STREAM).await;
                    match res {
                        Ok(_) => break,
                        Err(e) => {
                            error!("Error flushing transaction updates: {:?}", e);
                            sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
                metric! {
                    statsd_time!(
                        "transaction_chunk_processing_time",
                        chunk_start.elapsed().as_millis() as u64
                    );
                }
            }
        });
    join_all(transaction_futures).await;

    metric! {
        statsd_count!("transactions_processed", num_transactions as i64);
    }
    metric! {
        statsd_time!(
            "transactions_batch_processing_time",
            start.elapsed().as_millis() as u64
        );
    }
}

pub fn encode_account_update(pubkey: &Pubkey, account: Option<Account>, slot: u64) -> Vec<u8> {
    let fbb = flatbuffers::FlatBufferBuilder::new();
    let (lamports, owner, executable, rent_epoch, data) = match account {
        Some(account) => (
            account.lamports,
            account.owner,
            account.executable,
            account.rent_epoch,
            account.data,
        ),
        None => (0, solana_system_interface::program::id(), false, 0, vec![]),
    };
    let account_info = ReplicaAccountInfoV2 {
        pubkey: &pubkey.to_bytes(),
        lamports,
        owner: &owner.to_bytes(),
        executable,
        rent_epoch,
        data: &data,
        write_version: 0,
        txn_signature: None,
    };
    let is_startup = false;

    let fbb = serialize_account(fbb, &account_info, slot, is_startup);
    fbb.finished_data().to_vec()
}

pub async fn load_messenger(
    messenger_config: MessengerConfig,
) -> Result<Box<dyn Messenger>, MessengerError> {
    let mut messenger = select_messenger(messenger_config).await?;
    messenger.add_stream(ACCOUNT_STREAM).await?;
    messenger.add_stream(SLOT_STREAM).await?;
    messenger.add_stream(TRANSACTION_STREAM).await?;
    messenger.add_stream(BLOCK_STREAM).await?;
    messenger.add_stream(ACC_BACKFILL).await?;
    messenger.add_stream(TXN_BACKFILL).await?;
    messenger
        .set_buffer_size(ACCOUNT_STREAM, ACCOUNT_STREAM_SIZE)
        .await;
    messenger
        .set_buffer_size(TRANSACTION_STREAM, TRANSACTION_STREAM_SIZE)
        .await;
    messenger
        .set_buffer_size(BLOCK_STREAM, BLOCK_STREAM_SIZE)
        .await;
    messenger
        .set_buffer_size(TXN_BACKFILL, TRANSACTION_STREAM_SIZE)
        .await;
    Ok(messenger)
}

fn encode_transaction_update(tx: EncodedConfirmedTransactionWithStatusMeta) -> Vec<u8> {
    let fbb = flatbuffers::FlatBufferBuilder::new();
    let fbb = seralize_encoded_transaction_with_status(fbb, tx).unwrap();
    fbb.finished_data().to_vec()
}
