use std::{
    collections::{BTreeMap, HashMap, HashSet},
    str::FromStr,
    sync::{atomic::Ordering, Arc, OnceLock},
    time::Duration,
};

use async_stream::stream;
use cadence_macros::statsd_count;
use common::metric;
use futures::{pin_mut, Stream, StreamExt};
use log::info;
use nft_ingester::error::IngesterError;
use rayon::iter::IntoParallelIterator;
use solana_account_decoder::{UiAccountEncoding, UiDataSliceConfig};
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcBlockConfig},
    rpc_request::RpcError,
};

use rayon::iter::ParallelIterator;
use solana_program::pubkey;
use solana_commitment_config::CommitmentConfig;
use solana_sdk::{
    account::Account, pubkey::Pubkey,
    transaction::VersionedTransaction,
};
use solana_transaction_status::{
    option_serializer::OptionSerializer, EncodedTransactionWithStatusMeta, TransactionDetails,
    UiConfirmedBlock, UiTransactionEncoding, UiTransactionStatusMeta,
};
use tokio::sync::Mutex;

use super::utils::{DASBlock, RateLimiter};
use crate::{
    fetchers::utils::{BlockMetadata, Hash},
    monitor::{start_latest_slot_updater, LATEST_SLOT},
};

pub const BUBBLEGUM_PUBKEY: Pubkey = pubkey!("BGUMAp9Gq7iTEuizy4pqaxsTyUCBK68MDfK752saRPUY");
pub const DAS_ACCOUNTS: [Pubkey; 6] = [
    pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s"),
    pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
    pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
    pubkey!("META4s4fSmpkTbZoUsgC1oBnWB31vQcmnN8giPw51Zu"),
    pubkey!("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d"),
    // Agent Registry: AgentIdentity PDAs carry the agent_token mint.
    pubkey!("1DREGFgysWYxLnRnKQnwrxnJQeSMk2HmGaC6whw2B2p"),
];

const SKIPPED_BLOCK_ERRORS: [i64; 3] = [-32007, -32009, -32004];

const MAX_CONCURRENT_BLOCK_FETCHES: usize = 20;
const MAX_ACCOUNTS_PER_REQUEST: usize = 100;
const MAX_CONCURRENT_ACCOUNT_CALLS: usize = 1000;
const MAX_ACCOUNT_CALLS_PER_SECOND: u32 = 2500;

// Global account calls rate limiter

static RATE_LIMITER: OnceLock<Mutex<RateLimiter>> = OnceLock::new();

async fn acquite_account_rate_limiter() {
    let mut rate_limiter = RATE_LIMITER
        .get_or_init(|| {
            Mutex::new(RateLimiter::new(
                MAX_ACCOUNT_CALLS_PER_SECOND,
                Duration::from_secs(1),
            ))
        })
        .lock()
        .await;
    rate_limiter.acquire().await;
}

fn get_slot_stream(rpc_client: Arc<RpcClient>, start_slot: u64) -> impl Stream<Item = u64> {
    stream! {
        start_latest_slot_updater(rpc_client.clone()).await;
        let mut next_slot_to_fetch = start_slot;
        loop {
            if next_slot_to_fetch > LATEST_SLOT.load(Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
            }
            yield next_slot_to_fetch;
            next_slot_to_fetch += 1;
        }
    }
}

pub fn get_block_poller_stream(
    rpc_client: Arc<RpcClient>,
    mut last_indexed_slot: u64,
) -> impl Stream<Item = DASBlock> {
    stream! {
        let start_slot = match last_indexed_slot {
            0 => 0,
            last_indexed_slot => last_indexed_slot + 1
        };
        let slot_stream = get_slot_stream(rpc_client.clone(), start_slot);
        pin_mut!(slot_stream);
        let block_stream = slot_stream
            .map(|slot| {
                let rpc_client = rpc_client.clone();
                async move { fetch_block_with_account_data(rpc_client.clone(), slot).await }
            })
            .buffer_unordered(MAX_CONCURRENT_BLOCK_FETCHES);
        pin_mut!(block_stream);
        let mut block_cache: BTreeMap<u64, DASBlock> = BTreeMap::new();
        while let Some(block) = block_stream.next().await {
            if let Some(block) = block {
                block_cache.insert(block.block_metadata.slot, block);
            }
            let (blocks_to_index, last_indexed_slot_from_cache) = pop_cached_blocks_to_index(&mut block_cache, last_indexed_slot);
            last_indexed_slot = last_indexed_slot_from_cache;
            metric! {
                statsd_count!("rpc_block_emitted", blocks_to_index.len() as i64);
            }
            for block in blocks_to_index {
                yield block;
            }
        }
    }
}

fn pop_cached_blocks_to_index(
    block_cache: &mut BTreeMap<u64, DASBlock>,
    mut last_indexed_slot: u64,
) -> (Vec<DASBlock>, u64) {
    let mut blocks = Vec::new();
    loop {
        let min_slot = match block_cache.keys().min() {
            Some(&slot) => slot,
            None => break,
        };
        let block: &DASBlock = block_cache.get(&min_slot).unwrap();
        if block.block_metadata.parent_slot == last_indexed_slot {
            last_indexed_slot = block.block_metadata.slot;
            blocks.push(block.clone());
            block_cache.remove(&min_slot);
        } else if min_slot < last_indexed_slot {
            block_cache.remove(&min_slot);
        } else {
            break;
        }
    }
    (blocks, last_indexed_slot)
}

pub async fn fetch_block_with_account_data(client: Arc<RpcClient>, slot: u64) -> Option<DASBlock> {
    info!("Fetching block: {}", slot);
    loop {
        match client
            .get_block_with_config(
                slot,
                RpcBlockConfig {
                    encoding: Some(UiTransactionEncoding::Base64),
                    transaction_details: Some(TransactionDetails::Full),
                    rewards: None,
                    commitment: Some(CommitmentConfig::confirmed()),
                    // >= 1 or getBlock errors on blocks with v1 txs (SIMD-0296)
                    max_supported_transaction_version: Some(1),
                },
            )
            .await
        {
            Ok(block) => {
                metric! {
                    statsd_count!("rpc_block_fetched", 1);
                }

                log::info!(
                    "Recieved block from RPC - height: {:?}, transactions: {:?}, signatures: {:?}",
                    block.block_height,
                    block.transactions.as_ref().map(|txns| txns.len()),
                    block.signatures.as_ref().map(|sigs| sigs.len())
                );

                // Check for Helium transactions in the block
                let has_helium = block.transactions.as_ref().map_or(false, |txs| {
                    txs.iter().any(|tx| {
                        get_transaction_keys(tx.clone())
                            .iter()
                            .any(|k| k.to_string() == "memMa1HG4odAFmUbGWfPwS1WWfK95k99F2YTkGvyxZr")
                    })
                });

                if has_helium {
                    log::info!(
                        "HELIUM memMa1HG4: Fetched block {} with Helium transactions",
                        slot
                    );
                }

                return Some(
                    extend_block_with_account_data(client.clone(), block, slot)
                        .await
                        .unwrap(),
                );
            }
            Err(e) => {
                if let solana_client::client_error::ClientErrorKind::RpcError(
                    RpcError::RpcResponseError { code, .. },
                ) = *e.kind
                {
                    if SKIPPED_BLOCK_ERRORS.contains(&code) {
                        metric! {
                            statsd_count!("rpc_skipped_block", 1);
                        }
                        log::info!("Skipped block: {}", slot);
                        return None;
                    }
                }
                log::info!("Failed to fetch block: {}. {}", slot, e.to_string());
                metric! {
                    statsd_count!("rpc_block_fetch_failed", 1);
                }
            }
        }
    }
}

pub async fn extend_block_with_account_data(
    client: Arc<RpcClient>,
    block: UiConfirmedBlock,
    slot: u64,
) -> Result<DASBlock, IngesterError> {
    let block_metadata = BlockMetadata {
        slot,
        parent_slot: block.parent_slot,
        block_time: block.block_time.ok_or(IngesterError::ParsingError(
            "Missing block_time".to_string(),
        ))?,
        blockhash: Hash::try_from(block.blockhash.as_str()).map_err(|e| {
            IngesterError::ParsingError(format!("Failed to parse blockhash: {}", e))
        })?,
        parent_blockhash: Hash::try_from(block.previous_blockhash.as_str()).map_err(|e| {
            IngesterError::ParsingError(format!("Failed to parse previous_blockhash: {}", e))
        })?,
        block_height: block.block_height.ok_or(IngesterError::ParsingError(
            "Missing block_height".to_string(),
        ))?,
    };

    let prev_len = block.transactions.as_ref().map_or(0, |txs| txs.len());
    let transactions = filter_failed_transactions(block.transactions.unwrap_or(Vec::new()));
    log::info!(
        "Filtered failed {} txs from block {}",
        prev_len.saturating_sub(transactions.len()),
        block_metadata.slot
    );
    let modified_accounts = parse_modified_accounts_from_transactions(transactions.clone());
    let das_accounts = filter_das_accounts(
        client.clone(),
        slot,
        modified_accounts.into_iter().collect(),
    )
    .await;
    let account_stream = fetch_account_data_stream(client.clone(), slot, das_accounts, true).await;
    let mut accounts = HashMap::new();
    pin_mut!(account_stream);
    while let Some((account_data, slot)) = account_stream.next().await {
        for (account, pubkey) in account_data {
            accounts.insert(pubkey, (account, slot));
        }
    }
    let prev_len = transactions.len();
    let das_transactions = filter_non_bubblegum_transactions(transactions);
    log::info!(
        "Filtered non-bubblegum {} txs from block {}",
        prev_len.saturating_sub(das_transactions.len()),
        block_metadata.slot
    );
    Ok(DASBlock {
        block_metadata,
        das_accounts: accounts,
        das_transactions,
    })
}

pub fn parse_modified_accounts_from_transactions(
    transactions: Vec<EncodedTransactionWithStatusMeta>,
) -> HashSet<Pubkey> {
    transactions
        .into_par_iter()
        .flat_map(|tx| parse_modified_accounts_from_transaction(tx))
        .collect()
}

fn parse_modified_accounts_from_transaction(
    transaction: EncodedTransactionWithStatusMeta,
) -> Vec<Pubkey> {
    let EncodedTransactionWithStatusMeta {
        transaction, meta, ..
    } = transaction;

    // Skip undecodable txs (e.g. a newer transaction version) instead of panicking.
    let Some(versioned_transaction) = transaction.decode() else {
        log::error!("Failed to decode transaction; skipping account extraction");
        metric! {
            statsd_count!("rpc_transaction_decode_failed", 1);
        }
        return Vec::new();
    };
    let Some(meta) = meta else {
        log::error!("Transaction missing meta; skipping account extraction");
        return Vec::new();
    };
    parse_modified_accounts_from_versioned_transaction(versioned_transaction, meta)
}

fn parse_modified_accounts_from_versioned_transaction(
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

struct AccountMetadata {
    pubkey: Pubkey,
    owner: Option<Pubkey>,
    is_closed: bool,
}

async fn fetch_account_metadata(
    client: Arc<RpcClient>,
    min_slot: u64,
    accounts: Vec<Pubkey>,
) -> Vec<AccountMetadata> {
    let mut account_metadata = vec![];
    let account_data_stream =
        fetch_account_data_stream(client.clone(), min_slot, accounts, false).await;
    futures::pin_mut!(account_data_stream);
    while let Some((account_data, _)) = account_data_stream.next().await {
        for (account, pubkey) in account_data {
            account_metadata.push(AccountMetadata {
                pubkey: pubkey.clone(),
                owner: account.clone().map(|a| a.owner),
                is_closed: account.is_none(),
            });
        }
    }
    account_metadata
}

async fn filter_das_accounts(
    client: Arc<RpcClient>,
    min_slot: u64,
    accounts: Vec<Pubkey>,
) -> Vec<Pubkey> {
    let account_metadas = fetch_account_metadata(client.clone(), min_slot, accounts).await;
    account_metadas
        .into_iter()
        .filter(|account| match account.owner {
            Some(owner) => DAS_ACCOUNTS.contains(&owner),
            None => account.is_closed,
        })
        .map(|account| account.pubkey)
        .collect()
}

pub async fn fetch_account_data_stream(
    client: Arc<RpcClient>,
    min_slot: u64,
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
    let account_data_sream = account_chunk_stream
        .map(move |chunk| {
            let rpc_client = client.clone();
            async move {
                fetch_account_data_chunk_with_infinite_retries(
                    rpc_client,
                    min_slot,
                    &chunk,
                    include_data,
                )
                .await
            }
        })
        .buffer_unordered(MAX_CONCURRENT_ACCOUNT_CALLS);

    account_data_sream
}

async fn fetch_account_data_chunk_with_infinite_retries(
    client: Arc<RpcClient>,
    min_slot: u64,
    accounts: &[Pubkey],
    include_data: bool,
) -> (Vec<(Option<Account>, Pubkey)>, u64) {
    loop {
        acquite_account_rate_limiter().await;

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
                    min_context_slot: Some(min_slot),
                },
            )
            .await;
        match account_infos {
            Ok(account_infos) => {
                let context = account_infos.context;
                let account_infos = account_infos.value;
                let slot = context.slot;
                // A present account that fails to decode must not be mapped to
                // None: downstream publishes None as a closed account. Retry
                // the chunk instead.
                let decoded: Option<Vec<(Option<Account>, Pubkey)>> = account_infos
                    .into_iter()
                    .zip(accounts.iter().copied())
                    .map(|(ui_account, pubkey)| match ui_account {
                        None => Some((None, pubkey)),
                        Some(ui_account) => match decode_ui_account(ui_account) {
                            Some(account) => Some((Some(account), pubkey)),
                            None => {
                                log::error!("Failed to decode account {pubkey}; retrying chunk");
                                metric! {
                                    statsd_count!("rpc_account_decode_failed", 1);
                                }
                                None
                            }
                        },
                    })
                    .collect();
                match decoded {
                    Some(decoded) => return (decoded, slot),
                    None => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                }
            }
            Err(e) => {
                metric! {
                    statsd_count!("rpc_account_fetch_failed", 1);
                }
                let error_message = e.to_string();
                if error_message.contains("Minimum context slot has not been reached") {
                    metric! {
                        statsd_count!("rpc_account_fetch_failed_min_context_not_reached", 1);
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

fn decode_ui_account(ui_account: solana_account_decoder::UiAccount) -> Option<Account> {
    Some(Account {
        lamports: ui_account.lamports,
        data: ui_account.data.decode()?,
        owner: Pubkey::from_str(&ui_account.owner).ok()?,
        executable: ui_account.executable,
        rent_epoch: ui_account.rent_epoch,
    })
}

pub fn filter_failed_transactions(
    transactions: Vec<EncodedTransactionWithStatusMeta>,
) -> Vec<EncodedTransactionWithStatusMeta> {
    transactions
        .into_iter()
        .filter(|tx| tx.meta.is_some() && tx.meta.as_ref().unwrap().status.is_ok())
        .collect()
}

pub fn filter_non_bubblegum_transactions(
    txs: Vec<EncodedTransactionWithStatusMeta>,
) -> Vec<EncodedTransactionWithStatusMeta> {
    txs.into_iter()
        .filter(|tx| {
            get_transaction_keys(tx.clone())
                .iter()
                .any(|key| key == &BUBBLEGUM_PUBKEY)
        })
        .collect()
}

pub fn get_transaction_keys(tx: EncodedTransactionWithStatusMeta) -> Vec<Pubkey> {
    let Some(meta): Option<UiTransactionStatusMeta> = tx.meta else {
        log::error!("Transaction missing meta; skipping key extraction");
        return Vec::new();
    };
    // Get `UiTransaction` out of `EncodedTransactionWithStatusMeta`.
    let Some(ui_transaction): Option<VersionedTransaction> = tx.transaction.decode() else {
        log::error!("Failed to decode transaction; skipping key extraction");
        metric! {
            statsd_count!("rpc_transaction_decode_failed", 1);
        }
        return Vec::new();
    };
    let msg = ui_transaction.message;
    let atl_keys = msg.address_table_lookups();
    let mut account_keys = msg.static_account_keys().to_vec();

    if atl_keys.is_some() {
        if let OptionSerializer::Some(ad) = meta.loaded_addresses {
            for account in ad.writable {
                account_keys.push(Pubkey::from_str(&account).unwrap());
            }
            for account in ad.readonly {
                account_keys.push(Pubkey::from_str(&account).unwrap());
            }
        }
    }
    account_keys
}

pub fn is_bubblegum_transaction(tx: EncodedTransactionWithStatusMeta) -> bool {
    get_transaction_keys(tx).contains(&BUBBLEGUM_PUBKEY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_message::{v1, MessageHeader, VersionedMessage};
    use solana_sdk::signature::Signature;
    use solana_transaction_status::{EncodedTransaction, TransactionBinaryEncoding};

    // Regression test for SIMD-0296: the RPC decode path must handle v1
    // transactions, which use a different wire format than legacy/v0.
    #[test]
    fn decodes_v1_transaction() {
        let payer = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let message = v1::Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![payer, program],
            ..Default::default()
        };
        let transaction = VersionedTransaction {
            signatures: vec![Signature::default()],
            message: VersionedMessage::V1(message),
        };

        // wincode implements the true SIMD-0385 wire format (what validators and
        // RPC produce); the serde/bincode impl for V1 intentionally differs.
        let bytes = wincode::serialize(&transaction).unwrap();
        let encoded = EncodedTransaction::Binary(
            bs58::encode(&bytes).into_string(),
            TransactionBinaryEncoding::Base58,
        );

        let decoded = encoded.decode().expect("v1 transaction must decode");
        assert!(matches!(decoded.message, VersionedMessage::V1(_)));
        assert_eq!(decoded.message.static_account_keys(), &[payer, program]);
        assert!(decoded.message.address_table_lookups().is_none());
    }

    #[test]
    fn das_accounts_includes_agent_registry() {
        let agent_registry = pubkey!("1DREGFgysWYxLnRnKQnwrxnJQeSMk2HmGaC6whw2B2p");
        assert!(
            DAS_ACCOUNTS.contains(&agent_registry),
            "Agent Registry program must be in DAS_ACCOUNTS so its PDAs are published"
        );
    }
}
