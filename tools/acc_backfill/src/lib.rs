use log::error;
use plerkle_messenger::MessengerConfig;

use figment::{map, value::Value};
use {
    anyhow::Context,
    futures::{Stream, StreamExt},
    log::info,
    plerkle_messenger::ACC_BACKFILL,
    plerkle_serialization::{
        serializer::serialize_account, solana_geyser_plugin_interface_shims::ReplicaAccountInfoV2,
    },
    solana_account_decoder::{UiAccount, UiAccountEncoding, UiDataSliceConfig},
    solana_client::{
        nonblocking::rpc_client::RpcClient,
        rpc_config::{RpcAccountInfoConfig, RpcBlockConfig},
        rpc_request::{RpcError, RpcRequest},
        rpc_response::Response as RpcResponse,
    },
    solana_commitment_config::{CommitmentConfig, CommitmentLevel},
    solana_sdk::{
        account::Account,
        pubkey::Pubkey,
        transaction::VersionedTransaction,
    },
    solana_transaction_status::{
        option_serializer::OptionSerializer, EncodedTransactionWithStatusMeta, TransactionDetails,
        UiConfirmedBlock, UiTransactionEncoding, UiTransactionStatusMeta,
    },
    solana_system_interface,
    std::{collections::HashSet, str::FromStr, sync::Arc, time::Duration},
    tokio::sync::Mutex,
    txn_forwarder::rpc_tx_with_retries,
};

const SKIPPED_BLOCK_ERRORS: [i64; 2] = [-32007, -32009];
const MAX_CONCURRENT_BLOCK_FETCHES: usize = 500;
const MAX_CONCURRENT_ACCOUNT_FETCHES: usize = 100000;
const MAX_CONCURRENT_ACCOUNT_SENDS: usize = 10000;
const MAX_ACCOUNTS_PER_REQUEST: usize = 100;

/// fetch account from node and send it to redis
pub async fn fetch_and_send_account(
    pubkey: Pubkey,
    client: &RpcClient,
    messenger: &Arc<Mutex<Box<dyn plerkle_messenger::Messenger>>>,
    ok_to_fail: bool,
) -> anyhow::Result<()> {
    let fetch_result = fetch_account(pubkey, client).await;
    let (account, slot) = match fetch_result {
        Ok((account, slot)) => (account, slot),
        Err(e) => {
            if ok_to_fail {
                return Ok(());
            } else {
                return Err(anyhow::anyhow!("Failed to fetch account: {:?}", e));
            }
        }
    };
    send_account(pubkey, account, slot, messenger).await
}

/// fetch account and slot with retries
pub async fn fetch_account(pubkey: Pubkey, client: &RpcClient) -> anyhow::Result<(Account, u64)> {
    const CONFIG: RpcAccountInfoConfig = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64Zstd),
        commitment: Some(CommitmentConfig {
            commitment: CommitmentLevel::Finalized,
        }),
        data_slice: None,
        min_context_slot: None,
    };

    let response: RpcResponse<Option<UiAccount>> = rpc_tx_with_retries(
        client,
        RpcRequest::GetAccountInfo,
        serde_json::json!([pubkey.to_string(), CONFIG]),
        3,
        pubkey,
    )
    .await
    .with_context(|| format!("failed to get account {pubkey}"))?;

    let account: Account = response
        .value
        .ok_or_else(|| anyhow::anyhow!("failed to get account {pubkey}"))
        .and_then(|ui_account| {
            decode_ui_account(ui_account)
                .ok_or_else(|| anyhow::anyhow!("failed to parse account {pubkey}"))
        })?;

    Ok((account, response.context.slot))
}

/// send account data to redis
pub async fn send_account(
    pubkey: Pubkey,
    account: Account,
    slot: u64,
    messenger: &Arc<Mutex<Box<dyn plerkle_messenger::Messenger>>>,
) -> anyhow::Result<()> {
    let fbb = flatbuffers::FlatBufferBuilder::new();

    let account_info = ReplicaAccountInfoV2 {
        pubkey: &pubkey.to_bytes(),
        lamports: account.lamports,
        owner: &account.owner.to_bytes(),
        executable: account.executable,
        rent_epoch: account.rent_epoch,
        data: &account.data,
        write_version: 0,
        txn_signature: None,
    };
    let is_startup = false;

    let fbb = serialize_account(fbb, &account_info, slot, is_startup);
    let bytes = fbb.finished_data();

    messenger.lock().await.send(ACC_BACKFILL, bytes).await?;

    Ok(())
}

/// We do not reuse the same client for multiple requests, because it leads to degraded performance
fn load_rpc_client(rpc_uri: String) -> RpcClient {
    RpcClient::new_with_timeout_and_commitment(
        rpc_uri.clone(),
        Duration::from_secs(120),
        CommitmentConfig::confirmed(),
    )
}

pub async fn fetch_block(rpc_uri: String, slot: u64, retries: u64) -> Option<UiConfirmedBlock> {
    let mut attempt_counter = 0;
    loop {
        let client = load_rpc_client(rpc_uri.clone());
        match client
            .get_block_with_config(
                slot,
                RpcBlockConfig {
                    encoding: Some(UiTransactionEncoding::Base64),
                    transaction_details: Some(TransactionDetails::Full),
                    rewards: None,
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: Some(1),
                },
            )
            .await
        {
            Ok(block) => {
                return Some(block);
            }
            Err(e) => {
                if let solana_client::client_error::ClientErrorKind::RpcError(
                    RpcError::RpcResponseError { code, .. },
                ) = *e.kind
                {
                    if SKIPPED_BLOCK_ERRORS.contains(&code) {
                        return None;
                    }
                }
                attempt_counter += 1;
                if attempt_counter >= retries {
                    log::error!("Failed to fetch block {} after {} attempts. Last error: {}. Skipping block.", slot, retries, e.to_string());
                    return None;
                }
                log::warn!("Failed to fetch block {} (attempt {}/{}): {}. Retrying in 2s...", slot, attempt_counter, retries, e.to_string());
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

pub fn parse_accounts_from_block(block: UiConfirmedBlock) -> HashSet<Pubkey> {
    let UiConfirmedBlock { transactions, .. } = block;

    transactions
        .unwrap_or(Vec::new())
        .into_iter()
        .flat_map(|tx| parse_accounts_from_transaction(tx))
        .collect()
}

fn parse_accounts_from_transaction(transaction: EncodedTransactionWithStatusMeta) -> Vec<Pubkey> {
    let EncodedTransactionWithStatusMeta {
        transaction, meta, ..
    } = transaction;

    // Skip undecodable txs (e.g. a newer transaction version) instead of panicking.
    let Some(versioned_transaction): Option<VersionedTransaction> = transaction.decode() else {
        log::error!("Failed to decode transaction; skipping account extraction");
        return Vec::new();
    };
    let Some(meta) = meta else {
        log::error!("Transaction missing meta; skipping account extraction");
        return Vec::new();
    };
    parse_accounts_from_versioned_transaction(versioned_transaction, meta)
}

pub fn parse_accounts_from_versioned_transaction(
    versioned_transaction: VersionedTransaction,
    meta: UiTransactionStatusMeta,
) -> Vec<Pubkey> {
    let mut accounts: Vec<Pubkey> = versioned_transaction
        .message
        .static_account_keys()
        .iter()
        .enumerate()
        .filter(|&(i, _)| versioned_transaction.message.is_maybe_writable(i, None))
        .map(|(_, key)| key.clone())
        .collect();

    if versioned_transaction
        .message
        .address_table_lookups()
        .is_some()
    {
        if let OptionSerializer::Some(loaded_addresses) = meta.loaded_addresses.clone() {
            for address in loaded_addresses.writable.iter() {
                let pubkey = Pubkey::from_str(address).unwrap();
                accounts.push(pubkey);
            }
        }
    }
    accounts
}

pub async fn determine_accounts_in_block_range(
    rpc_uri: String,
    start_slot: u64,
    end_slot: u64,
) -> Vec<Pubkey> {
    let mut accounts = HashSet::new();

    let slot_stream = async_stream::stream! {
        for slot in start_slot..(end_slot + 1) {
            yield slot;
        }
    };
    futures::pin_mut!(slot_stream);

    let block_stream = slot_stream
        .map(|slot| {
            let rpc_uri = rpc_uri.clone();
            async move {
                let blocks_indexed = slot - start_slot;
                if blocks_indexed % 100 == 0 {
                    let blocks_indexed = slot - start_slot;
                    let blocks_total = end_slot - start_slot;
                    info!(
                        "Fetching block {}. Fetched {} blocks of {}",
                        slot, blocks_indexed, blocks_total
                    );
                }
                fetch_block(rpc_uri.clone(), slot, 10).await
            }
        })
        .buffer_unordered(MAX_CONCURRENT_BLOCK_FETCHES);
    futures::pin_mut!(block_stream);

    while let Some(block) = block_stream.next().await {
        if let Some(block) = block {
            accounts.extend(parse_accounts_from_block(block));
        }
    }

    accounts.into_iter().collect()
}

struct AccountMetadata {
    pubkey: Pubkey,
    owner: Option<Pubkey>,
    is_closed: bool,
}

async fn fetch_account_metadata(rpc_uri: String, accounts: Vec<Pubkey>) -> Vec<AccountMetadata> {
    let mut account_metadata = vec![];
    let total_accounts = accounts.len();
    let mut total_accounts_fetched = 0;
    let account_data_stream = fetch_account_data_stream(rpc_uri, accounts, false).await;
    futures::pin_mut!(account_data_stream);
    while let Some((account_data, _)) = account_data_stream.next().await {
        for (account, pubkey) in account_data {
            total_accounts_fetched += 1;
            if total_accounts_fetched % 1000 == 0 {
                info!(
                    "Fetched {} accounts out of {}",
                    total_accounts_fetched, total_accounts
                );
            }
            account_metadata.push(AccountMetadata {
                pubkey: pubkey.clone(),
                owner: account.clone().map(|a| a.owner),
                is_closed: account.is_none(),
            });
        }
    }
    account_metadata
}

pub async fn fetch_account_data_stream(
    rpc_uri: String,
    accounts: Vec<Pubkey>,
    include_data: bool,
) -> impl Stream<Item = (Vec<(Option<Account>, Pubkey)>, u64)> {
    let account_chunk_stream = async_stream::stream! {
        let mut account_chunk = vec![];
        for account in accounts.into_iter() {
            account_chunk.push(account.clone());
            if account_chunk.len() == MAX_ACCOUNTS_PER_REQUEST {
                yield account_chunk;
                account_chunk = vec![];
            }
        }
        yield account_chunk;
    };

    let max_concurrent_chunks = MAX_CONCURRENT_ACCOUNT_FETCHES / MAX_ACCOUNTS_PER_REQUEST;
    let account_data_sream = account_chunk_stream
        .map(move |chunk| {
            let rpc_uri = rpc_uri.clone();
            async move { fetch_account_data_chunk(rpc_uri.clone(), &chunk, include_data).await }
        })
        .buffer_unordered(max_concurrent_chunks);
    account_data_sream
}

async fn fetch_account_data_chunk(
    rpc_uri: String,
    accounts: &[Pubkey],
    include_data: bool,
) -> (Vec<(Option<Account>, Pubkey)>, u64) {
    let client = load_rpc_client(rpc_uri);
    let retries = 10;
    let mut tries = 0;

    while tries < retries {
        let account_infos = client
            .get_multiple_ui_accounts_with_config(
                &accounts,
                RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::Base64Zstd),
                    commitment: Some(CommitmentConfig::confirmed()),
                    data_slice: if !include_data {
                        Some(UiDataSliceConfig {
                            offset: 0,
                            length: 0,
                        })
                    } else {
                        None
                    },
                    min_context_slot: None,
                },
            )
            .await;

        if let Ok(account_infos) = account_infos {
            let context = account_infos.context;
            let account_infos = account_infos.value;
            let slot = context.slot;
            // A present account that fails to decode must not be mapped to None
            // (None means missing/closed to callers); retry the chunk instead.
            let decoded: Option<Vec<(Option<Account>, Pubkey)>> = account_infos
                .into_iter()
                .zip(accounts.iter().copied())
                .map(|(ui_account, pubkey)| match ui_account {
                    None => Some((None, pubkey)),
                    Some(ui_account) => match decode_ui_account(ui_account) {
                        Some(account) => Some((Some(account), pubkey)),
                        None => {
                            log::error!("Failed to decode account {pubkey}; retrying chunk");
                            None
                        }
                    },
                })
                .collect();
            if let Some(decoded) = decoded {
                return (decoded, slot);
            }
            tries += 1;
            tokio::time::sleep(Duration::from_secs(2)).await;
        } else {
            tries += 1;
            if tries >= retries {
                log::error!("Failed to fetch account data chunk after {} attempts. Last error: {:?}. Returning empty results.", retries, account_infos);
                // Return empty accounts as None to indicate failure
                return (
                    accounts.iter().map(|pk| (None, pk.clone())).collect(),
                    0,
                );
            }
            log::warn!("Failed to fetch account data chunk (attempt {}/{}). Retrying in 2s...", tries, retries);
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
    // This should never be reached due to the logic above, but just in case
    log::error!("Failed to fetch account data after retries. Returning empty results.");
    (
        accounts.iter().map(|pk| (None, pk.clone())).collect(),
        0,
    )
}

fn decode_ui_account(ui_account: UiAccount) -> Option<Account> {
    Some(Account {
        lamports: ui_account.lamports,
        data: ui_account.data.decode()?,
        owner: Pubkey::from_str(&ui_account.owner).ok()?,
        executable: ui_account.executable,
        rent_epoch: ui_account.rent_epoch,
    })
}

async fn filter_das_accounts(rpc_uri: String, accounts: Vec<Pubkey>) -> Vec<Pubkey> {
    let account_metadas = fetch_account_metadata(rpc_uri, accounts).await;
    let das_owners = vec![
        Pubkey::from_str("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s").unwrap(),
        Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap(),
        Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap(),
        Pubkey::from_str("META4s4fSmpkTbZoUsgC1oBnWB31vQcmnN8giPw51Zu").unwrap(),
        Pubkey::from_str("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d").unwrap(),
    ];
    account_metadas
        .into_iter()
        .filter(|account| match account.owner {
            Some(owner) => das_owners.contains(&owner),
            None => account.is_closed,
        })
        .map(|account| account.pubkey)
        .collect()
}

pub async fn send_account_stream(
    account_stream: impl Stream<Item = (Vec<(Option<Account>, Pubkey)>, u64)>,
    redis_url: String,
) {
    let mut total_account_sent = 0;

    let sent_stream = account_stream
        .map(|(account_data, slot)| {
            let redis_url = redis_url.clone();
            async move {
                let num_accounts = account_data.len();
                let messenger = load_redis_messenger(redis_url.clone()).await;
                let mut send_account_futures = vec![];
                for (account, pubkey) in account_data {
                    let account = account.unwrap_or_else(|| Account {
                        lamports: 0,
                        owner: solana_system_interface::program::id(),
                        executable: false,
                        rent_epoch: 0,
                        data: vec![],
                    });
                    send_account_futures.push(send_account(pubkey, account, slot, &messenger));
                }
                let results = futures::future::join_all(send_account_futures).await;
                let mut error_count = 0;
                for result in results {
                    if let Err(e) = result {
                        log::error!("Error sending account: {:?}", e);
                        error_count += 1;
                    }
                }
                if error_count > 0 {
                    log::warn!("Failed to send {} accounts in this batch, continuing anyway", error_count);
                }
                if let Err(e) = messenger
                    .lock()
                    .await
                    .flush(ACC_BACKFILL)
                    .await
                {
                    log::error!("Failed to flush to Redis: {:?}", e);
                }

                num_accounts
            }
        })
        .buffer_unordered(MAX_CONCURRENT_ACCOUNT_SENDS / MAX_ACCOUNTS_PER_REQUEST);
    futures::pin_mut!(sent_stream);

    while let Some(num_accounts) = sent_stream.next().await {
        for _ in 0..num_accounts {
            total_account_sent += 1;
            if total_account_sent % 1000 == 0 {
                let messenger = load_redis_messenger(redis_url.clone()).await;
                while fetch_stream_size_with_infinite_retries(messenger.clone(), ACC_BACKFILL).await
                    > 100_000
                {
                    info!("Stream size is too large, sleeping for 1 second");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }

                info!("Sent {} accounts", total_account_sent);
            }
        }
    }
}

pub async fn fetch_stream_size_with_infinite_retries(
    messenger: Arc<Mutex<Box<dyn plerkle_messenger::Messenger>>>,
    stream_key: &'static str,
) -> u64 {
    loop {
        let res = messenger.lock().await.stream_size(stream_key).await;
        match res {
            Ok(size) => return size,
            Err(e) => {
                error!("Error fetching stream size: {:?}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

pub async fn send_das_accounts_in_account_range(
    rpc_uri: String,
    redis_url: String,
    start_slot: u64,
    end_slot: u64,
) {
    let accounts = determine_accounts_in_block_range(rpc_uri.clone(), start_slot, end_slot).await;
    let accounts = filter_das_accounts(rpc_uri.clone(), accounts).await;
    let account_data_stream = fetch_account_data_stream(rpc_uri.clone(), accounts, true).await;
    futures::pin_mut!(account_data_stream);

    send_account_stream(account_data_stream, redis_url.clone()).await;
}

pub async fn load_redis_messenger(
    redis_url: String,
) -> Arc<Mutex<Box<dyn plerkle_messenger::Messenger>>> {
    let config_wrapper = Value::from(map! {
        "redis_connection_str" => redis_url,
        "pipeline_size_bytes" => 1u128.to_string(),
    });
    let config = config_wrapper.into_dict().unwrap();
    let messenger_config = MessengerConfig {
        messenger_type: plerkle_messenger::MessengerType::Redis,
        connection_config: config,
    };
    let mut messenger = plerkle_messenger::select_messenger(messenger_config)
        .await
        .unwrap();
    messenger
        .add_stream_without_configuring_consumer_group(ACC_BACKFILL)
        .await
        .unwrap();
    messenger
        .set_buffer_size(ACC_BACKFILL, 10000000000000000)
        .await;

    Arc::new(Mutex::new(messenger))
}
