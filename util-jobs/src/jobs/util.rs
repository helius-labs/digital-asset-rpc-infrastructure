use crate::{error::DasJobErr, metric};
use acc_forwarder::{
    fetch_account, fetch_and_send_account, get_token_largest_account, send_account,
};
use cadence_macros::statsd_time;
use cadence_macros::{is_global_default_set, statsd_count};
use digital_asset_types::dao::{asset, FullAsset};
use futures::stream::{FuturesUnordered, StreamExt};
use log::{debug, error, warn};
use mpl_token_metadata::accounts::Metadata;
use plerkle_messenger::Messenger;
use sea_orm::{DatabaseConnection, EntityTrait};
use serde_json::{json, Value};
use solana_account_decoder::UiAccountEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_client::rpc_request::RpcRequest;
use solana_rpc_client_api::client_error::Error;
use solana_commitment_config::{CommitmentConfig, CommitmentLevel};
use solana_sdk::pubkey::Pubkey;
use spl_token::state::GenericTokenAccount;
use std::time::Instant;
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::{sync::Mutex, task};
use tokio_retry::strategy::{jitter, ExponentialBackoff};

enum AccountType {
    Mint,
    Metadata,
    Token,
}
struct Account {
    account_type: AccountType,
    pubkey: Pubkey,
}

/// Forward mint account, metadata account, and associated token account to the messenger.
pub async fn forward_mint(
    mint_address: String,
    client: &RpcClient,
    messenger: &Arc<Mutex<Box<dyn Messenger>>>,
) -> Result<(), DasJobErr> {
    let mint = Pubkey::from_str(&mint_address)?;
    let (metadata, _) = Metadata::find_pda(&mint);
    let token = get_token_largest_account(client, mint).await;

    match token {
        Ok(token) => {
            let mint_account = Account {
                account_type: AccountType::Mint,
                pubkey: mint,
            };
            let metadata_account = Account {
                account_type: AccountType::Metadata,
                pubkey: metadata,
            };
            let token_account = Account {
                account_type: AccountType::Token,
                pubkey: token,
            };
            for acc in &[metadata_account, mint_account, token_account] {
                let x = fetch_and_send_account(acc.pubkey, client, messenger, false).await;
                match x {
                    Ok(_) => debug!("Successfully forwarded account: {}", acc.pubkey),
                    Err(e) => {
                        error!(
                            "Failed to forward account {} for mint {}, {:?}",
                            acc.pubkey, mint, e
                        );
                        match acc.account_type {
                            AccountType::Metadata => return Ok(()), // burnt mints with no metadata should not error
                            _ => return Err(DasJobErr::AccountForwardError(e.to_string())),
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!("Failed to find token account for mint {}, {:?}", mint, e);
            return Err(DasJobErr::AccountForwardError(e.to_string()));
        }
    }
    Ok(())
}

/// Concurrency checks and re-indexes all assets
pub async fn reindex_assets_concurrency(
    conn: Arc<DatabaseConnection>,
    rpc_url: &String,
    messenger: &Arc<Mutex<Box<dyn Messenger>>>,
    assets: Vec<FullAsset>,
    tag_key: String,
    tag_value: String,
    _concurrency: usize,
) -> Result<(), DasJobErr> {
    // Remove semaphore when debugging mem leak
    // let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut futures = FuturesUnordered::new();
    for asset in assets {
        let asset_id = bs58::encode(asset.asset.id.clone()).into_string();
        // let sema_clone = semaphore.clone();
        let messenger_clone = Arc::clone(messenger);
        let client_clone = RpcClient::new(rpc_url.clone());
        let tag_key = tag_key.clone();
        let tag_value = tag_value.clone();
        let conn = conn.clone();
        futures.push(task::spawn(async move {
            // let _permit = sema_clone.acquire().await; // Controls concurrency
            match check_and_reindex_asset(
                conn,
                asset,
                asset_id,
                &client_clone,
                &messenger_clone,
                &tag_key,
                &tag_value,
            )
            .await
            {
                Ok(_) => {
                    statsd_count_w_tag("check_success", 1 as i64, &tag_key, &tag_value);
                }
                Err(e) => {
                    error!("Error reindexing asset: {:?}", e);
                    statsd_count_w_tag("check_error", 1 as i64, &tag_key, &tag_value);
                }
            }
        }));
    }
    while let Some(_) = futures.next().await {} // Wait for all tasks to complete
    Ok(())
}

/// Checks if an indexed asset has incorrect ownership.
/// If yes, it will push the latest token account to the indexer.
pub async fn check_and_reindex_asset(
    conn: Arc<DatabaseConnection>,
    asset: FullAsset,
    asset_id: String,
    client: &RpcClient,
    messenger: &Arc<Mutex<Box<dyn Messenger>>>,
    tag_key: &str,
    tag_value: &str,
) -> Result<(), DasJobErr> {
    debug!("Checking asset: {}", asset_id);
    let mut indexed_owner = asset
        .asset
        .owner
        .map(|o| bs58::encode(o).into_string())
        .unwrap_or("NO_OWNER".into());
    let mint = Pubkey::from_str(&asset_id).unwrap();

    let max_retries = 3;
    for retry in 0..(max_retries + 1) {
        let token_account = get_token_largest_account(&client, mint)
            .await
            .map_err(|e| {
                DasJobErr::ReindexError(format!(
                    "Failed to fetch largest token accounts for asset {}: {}",
                    asset_id,
                    e.to_string()
                ))
            })?;
        let (ta, slot) = fetch_account(token_account, &client).await.map_err(|e| {
            DasJobErr::ReindexError(format!(
                "Failed to get account info for token account for asset {}: {}",
                asset_id,
                e.to_string()
            ))
        })?;
        let actual_owner =
            match <spl_token::state::Account as GenericTokenAccount>::unpack_account_owner(&ta.data)
            {
                Some(owner) => Ok(bs58::encode(owner).into_string()),
                None => Err(DasJobErr::ReindexError(format!(
                    "Failed to parse token account for mint {}",
                    asset_id,
                ))),
            }?;

        if actual_owner != indexed_owner {
            warn!(
                "Owner mismatch. Indexed owner for mint {} was {} but current owner is {} (retry = {}).",
                asset_id, indexed_owner, actual_owner, retry
            );

            if let Some(asset) = asset::Entity::find_by_id(mint.to_bytes().to_vec())
                .one(conn.as_ref())
                .await?
            {
                indexed_owner = asset
                    .owner
                    .map(|o| bs58::encode(o).into_string())
                    .unwrap_or("NO_OWNER".into());
            }

            // Due to indexing delay the owner may have not yet updated.
            if retry < max_retries {
                statsd_count_w_tag("incorrect_owner", 1 as i64, tag_key, tag_value);
                tokio::time::sleep(Duration::from_secs(10)).await;
            } else {
                statsd_count_w_tag(
                    "incorrect_owner_after_retries",
                    1 as i64,
                    tag_key,
                    tag_value,
                );
                send_account(token_account, ta, slot, &messenger)
                    .await
                    .map_err(|e| {
                        DasJobErr::ReindexError(format!(
                            "Failed to send token account to redis for asset {}: {}",
                            asset_id,
                            e.to_string()
                        ))
                    })?
            }
        } else {
            break;
        }
    }

    Ok(())
}

/// Simple utility.
/// With CW we need to publish both with and without tag if we want aggregated metrics.
pub fn statsd_count_w_tag(metric: &str, count: i64, tag_key: &str, tag_value: &str) {
    let full_metric = format!("das_job.reindex_{}.{}", tag_key, metric);
    let full_metric_ref = full_metric.as_str();
    metric! {
        statsd_count!(full_metric_ref, count);
    }
    metric! {
        statsd_count!(full_metric_ref, count, tag_key => tag_value);
    }
}

/// Asynchronously retrieves the closure status of an account from the RPC client.
/// Returns `Ok(Some(slot))` if the account is closed, `Ok(None)` if not closed, or `Err(DasJobErr::RpcError)` on error.
pub async fn get_account_closure_status(
    client: Arc<RpcClient>,
    pubkey: Pubkey,
) -> Result<Option<i64>, DasJobErr> {
    // Copied config values from get_account_with_commitment. We do not use that function directly
    // it re-emits all the errors as `AccountNotFound` errors, which is misleading.
    let config = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64Zstd),
        commitment: Some(CommitmentConfig {
            commitment: CommitmentLevel::Confirmed,
        }),
        data_slice: None,
        min_context_slot: None,
    };
    let start = Instant::now();
    let response: Result<Value, Error> = client
        .send(
            RpcRequest::GetAccountInfo,
            json!([pubkey.to_string(), config]),
        )
        .await;

    metric! {
        statsd_time!(
            "account_closure_check_latency",
            start.elapsed()
        );
    }

    match response {
        Ok(response_json) => {
            let slot = response_json
                .get("context")
                .and_then(|c| c.get("slot"))
                .and_then(|v| v.as_i64())
                .ok_or(DasJobErr::RpcError(format!(
                    "Error getting slot info for {pubkey}."
                )))?;

            response_json
                .get("value")
                .map(|value| if value.is_null() { Some(slot) } else { None })
                .ok_or(DasJobErr::RpcError(format!(
                    "Error getting account info for {pubkey}."
                )))
        }
        Err(e) => {
            let e = Err(DasJobErr::RpcError(format!(
                "Error getting account info for {pubkey}. {e}"
            )));
            debug!("{:?}", e);
            e
        }
    }
}

// We really don't want any errors in a multi day backfill
pub fn get_aggressive_retry_strategy() -> impl Iterator<Item = Duration> {
    ExponentialBackoff::from_millis(1000)
        .max_delay(Duration::from_secs(30))
        .map(jitter)
        .take(5)
}

pub fn parse_checkpoint(checkpoint: Option<String>) -> Result<Option<Vec<u8>>, DasJobErr> {
    let checkpoint = match checkpoint {
        Some(c) => Some(
            bs58::decode(c.as_str())
                .into_vec()
                .map_err(|e| DasJobErr::ParsePubkeyError(e.to_string()))?,
        ),
        None => None,
    };
    Ok(checkpoint)
}

#[tokio::test]
async fn test_is_account_closed() {
    // Not importing from config to avoid having to input so many env vars
    let rpc_url =
        std::env::var("RPC_URL").expect("RPC_URL");
    let mock_client = RpcClient::new(rpc_url.clone());
    let client_ref = Arc::new(mock_client);

    let key_base_58_closed = "11z6svKPS13qp146nCPsjJf1E8gjgPWx5JWkMQdNzq";
    let key = Pubkey::from_str(key_base_58_closed).unwrap();
    let closure_status = get_account_closure_status(client_ref.clone(), key)
        .await
        .unwrap();
    assert!(closure_status.is_some());

    let key_base_58_open = "EGiWZhNk3vUNJr35MbL2tY5YD6D81VVZghR2LgEFyXZh";
    let key: Pubkey = Pubkey::from_str(key_base_58_open).unwrap();
    let closure_status = get_account_closure_status(client_ref.clone(), key)
        .await
        .unwrap();
    assert!(closure_status.is_none());
}
