use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};

use async_stream::stream;
use cadence_macros::statsd_count;
use common::metric;
use futures::future::{select, Either};
use futures::sink::SinkExt;
use futures::{pin_mut, Stream, StreamExt};
use log::{error, info};
use rand::distr::Alphanumeric;
use rand::Rng;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::{
    EncodableWithMeta, EncodedTransactionWithStatusMeta, TransactionWithStatusMeta,
    UiTransactionEncoding, VersionedTransactionWithStatusMeta,
};
use tokio::time::sleep;
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcBuilderResult, GeyserGrpcClient};
use super::grpc_convert::{self as convert_from, create_account};
use yellowstone_grpc_proto::geyser::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest, SubscribeRequestPing,
};
use yellowstone_grpc_proto::geyser::{
    SubscribeRequestFilterBlocks, SubscribeUpdateAccountInfo, SubscribeUpdateBlock,
    SubscribeUpdateTransactionInfo,
};

use crate::fetchers::poller::get_block_poller_stream;
use crate::monitor::{start_latest_slot_updater, HEALTH_CHECK_SLOT_DISTANCE, LATEST_SLOT};

use super::poller::{
    filter_failed_transactions, filter_non_bubblegum_transactions,
    parse_modified_accounts_from_transactions, DAS_ACCOUNTS,
};
use super::utils::{BlockMetadata, DASBlock, Hash};

pub const NUMBER_OF_SLOTS_TO_CACHE: usize = 20;

pub fn get_grpc_stream_with_rpc_fallback(
    endpoint: String,
    auth_header: String,
    rpc_client: Arc<RpcClient>,
    mut last_indexed_slot: u64,
) -> impl Stream<Item = DASBlock> {
    stream! {
        start_latest_slot_updater(rpc_client.clone()).await;
        let grpc_stream = get_grpc_block_stream(endpoint, auth_header, Some(last_indexed_slot));
        pin_mut!(grpc_stream);
        let mut rpc_poll_stream:  Option<Pin<Box<dyn Stream<Item = DASBlock> + Send>>> = Some(
            Box::pin(get_block_poller_stream(
                rpc_client.clone(),
                last_indexed_slot,
            ))
        );

        // Await either the gRPC stream or the RPC block fetching
        loop {
            match rpc_poll_stream.as_mut() {
                Some(rpc_poll_stream_value) => {
                    match select(grpc_stream.next(), rpc_poll_stream_value.next()).await {
                        Either::Left((Some(grpc_block), _)) => {
                            let grpc_block = grpc_block;
                            let slot = grpc_block.block_metadata.slot;
                            if grpc_block.block_metadata.parent_slot == last_indexed_slot {
                                last_indexed_slot = grpc_block.block_metadata.slot;
                                yield grpc_block;
                                metric! {
                                    statsd_count!("grpc_block_emitted", 1);
                                }
                                if is_healthy(slot) {
                                    info!("Switching to gRPC block fetching since Photon is up-to-date");
                                    rpc_poll_stream = None;
                                }
                            }
                        }
                        Either::Left((None, _)) => {
                            panic!("gRPC stream ended unexpectedly");
                        }
                        Either::Right((Some(rpc_block), _)) => {
                            let parent_slot = rpc_block.block_metadata.parent_slot;
                            let last_slot = rpc_block.block_metadata.slot;
                            if parent_slot == last_indexed_slot {
                                last_indexed_slot = last_slot;
                                yield rpc_block;
                                metric! {
                                    statsd_count!("rpc_block_indexed", 1);
                                }
                            } else if poller_is_desynced(last_slot, last_indexed_slot) {
                                info!(
                                    "Poller desynced (block {last_slot}, parent {parent_slot}, cursor {last_indexed_slot}). Restarting poller from cursor"
                                );
                                metric! {
                                    statsd_count!("rpc_poller_resync", 1);
                                }
                                rpc_poll_stream = Some(Box::pin(get_block_poller_stream(
                                    rpc_client.clone(),
                                    last_indexed_slot,
                                )));
                            }
                        }
                        Either::Right((None, _)) => {
                            panic!("RPC stream ended unexpectedly");
                        }
                    }
                }
                None => {
                    let block = match tokio::time::timeout(Duration::from_secs(5), grpc_stream.next()).await {
                        Ok(Some(block)) => block,
                        Ok(None) => panic!("gRPC stream ended unexpectedly"),
                        Err(_) => {
                            metric! {
                                statsd_count!("grpc_timeout", 1);
                            }
                            info!("gRPC stream timed out, enabling RPC block fetching");
                            rpc_poll_stream = Some(Box::pin(get_block_poller_stream(
                                rpc_client.clone(),
                                last_indexed_slot,
                            )));
                            continue;
                        }
                    };
                    let slot = block.block_metadata.slot;
                    if block.block_metadata.parent_slot == last_indexed_slot {
                        last_indexed_slot = block.block_metadata.slot;
                        yield block;
                    } else {
                        metric! {
                            statsd_count!("grpc_out_of_order", 1);
                        }
                        info!("Switching to RPC block fetching");
                        rpc_poll_stream = Some(Box::pin(get_block_poller_stream(
                            rpc_client.clone(),
                            last_indexed_slot,
                        )));
                        continue;
                    }
                    if !is_healthy(slot) && rpc_poll_stream.is_none() {
                        info!("gRPC is unhealthy. Enabling RPC block fetching");
                        metric! {
                            statsd_count!("grpc_stale", 1);
                        }
                        rpc_poll_stream = Some(Box::pin(get_block_poller_stream(
                            rpc_client.clone(),
                            last_indexed_slot,
                        )));
                    }
                }
            }


        }
    }
}

fn is_healthy(slot: u64) -> bool {
    (LATEST_SLOT.load(Ordering::SeqCst) as i64 - slot as i64) <= HEALTH_CHECK_SLOT_DISTANCE as i64
}

/// A non-chaining block past the cursor means the poller has jumped over the
/// cursor and can never chain onto it again; it must be rebuilt from the cursor.
fn poller_is_desynced(block_slot: u64, last_indexed_slot: u64) -> bool {
    block_slot > last_indexed_slot
}

pub fn get_grpc_block_stream(
    endpoint: String,
    auth_header: String,
    mut last_indexed_slot: Option<u64>,
) -> impl Stream<Item = DASBlock> {
    stream! {
        loop {
            let mut grpc_tx;
            let mut grpc_rx;
            {
                let grpc_client =
                    build_geyser_client(endpoint.clone(), auth_header.clone()).await;
                if let Err(e) = grpc_client {
                    error!("Error connecting to gRPC, waiting one second then retrying connect: {}", e);
                    metric! {
                        statsd_count!("grpc_connect_error", 1);
                    }
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
                let subscription = grpc_client
                    .unwrap()
                    .subscribe_with_request(Some(get_block_and_slot_subscribe_request(last_indexed_slot.map(|slot| slot + 1))))
                    .await;
                if let Err(e) = subscription {
                    error!("Error subscribing to gRPC stream, waiting one second then retrying connect: {}", e);
                    metric! {
                        statsd_count!("grpc_subscribe_error", 1);
                    }
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
                (grpc_tx, grpc_rx) = subscription.unwrap();
            }
            while let Some(message) = grpc_rx.next().await {
                match message {
                    Ok(message) => match message.update_oneof {
                        Some(UpdateOneof::Block(block)) => {
                            metric! {
                                statsd_count!("grpc_block_received", 1);
                            }
                            last_indexed_slot = Some(block.slot);
                            yield parse_block(block);
                        }
                        Some(UpdateOneof::Ping(_)) => {
                            // This is necessary to keep load balancers that expect client pings alive. If your load balancer doesn't
                            // require periodic client pings then this is unnecessary
                            let ping = grpc_tx.send(ping()).await;
                            if let Err(e) = ping {
                                error!("Error sending ping: {}", e);
                                metric! {
                                    statsd_count!("grpc_ping_error", 1);
                                }
                                break;
                            }
                        }
                        Some(UpdateOneof::Pong(_)) => {}
                        _ => {
                            error!("Unknown message: {:?}", message);
                        },
                    },
                    Err(error) => {
                        error!(
                            "error in block subscribe, resubscribing in 1 second: {error:?}"
                        );
                        metric! {
                            statsd_count!("grpc_resubscribe", 1);
                        }
                        break;
                    }
                }
            }
        sleep(Duration::from_secs(1)).await;
        }
    }
}

async fn build_geyser_client(
    endpoint: String,
    auth_header: String,
) -> GeyserGrpcBuilderResult<GeyserGrpcClient> {
    GeyserGrpcClient::build_from_shared(endpoint)?
        .x_token(Some(auth_header))?
        .connect_timeout(Duration::from_secs(10))
        .max_decoding_message_size(100 * 8388608)
        .tls_config(ClientTlsConfig::new().with_native_roots())?
        .timeout(Duration::from_secs(10))
        .http2_keep_alive_interval(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(10))
        .keep_alive_while_idle(true)
        .connect()
        .await
}

fn generate_random_string(len: usize) -> String {
    rand::rng()
        .sample_iter(Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn get_block_and_slot_subscribe_request(from_slot: Option<u64>) -> SubscribeRequest {
    info!("Subscribing to gRPC block stream from slot {}", from_slot.unwrap_or(0));
    SubscribeRequest {
        blocks: HashMap::from_iter(vec![(
            generate_random_string(20),
            SubscribeRequestFilterBlocks {
                account_include: vec![],
                include_transactions: Some(true),
                include_accounts: Some(true),
                include_entries: Some(false),
                ..Default::default()
            },
        )]),
        commitment: Some(CommitmentLevel::Confirmed.into()),
        from_slot,
        ..Default::default()
    }
}

fn ping() -> SubscribeRequest {
    SubscribeRequest {
        ping: Some(SubscribeRequestPing { id: 1 }),
        ..Default::default()
    }
}

fn parse_block(block: SubscribeUpdateBlock) -> DASBlock {
    let metadata = BlockMetadata {
        slot: block.slot,
        parent_slot: block.parent_slot,
        block_time: block.block_time.unwrap().timestamp,
        blockhash: Hash::try_from(block.blockhash.as_str()).unwrap(),
        parent_blockhash: Hash::try_from(block.parent_blockhash.as_str()).unwrap(),
        block_height: block.block_height.unwrap().block_height,
    };
    log::info!(
        "Received block from gRPC - height: {}, transactions: {}, accounts: {}",
        metadata.slot,
        block.transactions.len(),
        block.accounts.len()
    );
    let transactions: Vec<EncodedTransactionWithStatusMeta> = block
        .transactions
        .into_par_iter()
        .map(parse_transaction)
        .collect();
    let prev_len = transactions.len();
    let transactions = filter_failed_transactions(transactions);
    log::info!(
        "Filtered {} failed transactions in slot {}",
        prev_len.saturating_add(transactions.len()),
        metadata.slot
    );
    let modified_accounts = parse_modified_accounts_from_transactions(transactions.clone());
    let prev_len = transactions.len();
    let transactions = filter_non_bubblegum_transactions(transactions);
    log::info!(
        "Filtered {} non-bubblegum transactions in slot {}",
        prev_len.saturating_add(transactions.len()),
        metadata.slot
    );

    let accounts: HashMap<Pubkey, (Option<Account>, u64)> = block
        .accounts
        .into_par_iter()
        .filter_map(|account| {
            let (pubkey, account) = parse_account(account);
            if !modified_accounts.contains(&pubkey) {
                return None;
            }
            if DAS_ACCOUNTS.contains(&account.owner)
                || account.owner == solana_system_interface::program::id()
            {
                if account.owner == solana_system_interface::program::id() {
                    if account.lamports == 0 {
                        return Some((pubkey, (Some(account), block.slot)));
                    } else {
                        None
                    }
                } else {
                    Some((pubkey, (Some(account), block.slot)))
                }
            } else {
                None
            }
        })
        .collect();

    DASBlock {
        block_metadata: metadata,
        das_transactions: transactions,
        das_accounts: accounts,
    }
}

fn parse_transaction(
    transaction: SubscribeUpdateTransactionInfo,
) -> EncodedTransactionWithStatusMeta {
    let tx_with_meta = convert_from::create_tx_with_meta(transaction).unwrap();
    match tx_with_meta {
        TransactionWithStatusMeta::Complete(tx_with_meta) => {
            let VersionedTransactionWithStatusMeta { transaction, meta } = tx_with_meta;
            let encoded_transaction =
                transaction.encode_with_meta(UiTransactionEncoding::Base64, &meta);
            EncodedTransactionWithStatusMeta {
                transaction: encoded_transaction,
                meta: Some(meta.into()),
                version: None,
            }
        }
        TransactionWithStatusMeta::MissingMetadata(tx) => {
            panic!("Transaction metadata is missing: {:?}", tx);
        }
    }
}

fn parse_account(account: SubscribeUpdateAccountInfo) -> (Pubkey, Account) {
    create_account(account).unwrap()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn poller_behind_or_at_cursor_is_a_benign_race() {
        assert!(!poller_is_desynced(99, 100));
        assert!(!poller_is_desynced(100, 100));
    }

    #[test]
    fn poller_past_cursor_requires_resync() {
        assert!(poller_is_desynced(101, 100));
        assert!(poller_is_desynced(2_600, 100));
    }

    #[tokio::test]
    #[ignore]
    async fn test_grpc_stream() {
        let stream = get_grpc_block_stream(
            std::env::var("GRPC_URL").expect("GRPC_URL"),
            std::env::var("GRPC_AUTH_HEADER").expect("GRPC_AUTH_HEADER"),
            None,
        );
        pin_mut!(stream);
        start_latest_slot_updater(Arc::new(RpcClient::new(
            std::env::var("RPC_URL").expect("RPC_URL")
                .to_string(),
        )))
        .await;
        while let Some(block) = stream.next().await {
            let slot = block.block_metadata.slot;
            let start = Instant::now();
            let end = Instant::now();
            let duration = end.duration_since(start);
            println!("Duration: {:?}", duration);
            let latest_slot = LATEST_SLOT.load(Ordering::SeqCst);
            let diff = if latest_slot > slot {
                latest_slot - slot
            } else {
                0
            };
            println!(
                "Diff: {:?}. Slot: {:?}. Latest slot: {:?}",
                diff, slot, latest_slot
            );
        }
    }
}
