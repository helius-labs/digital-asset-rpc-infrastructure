use log::{error, info};

// Import shared utilities from acc_backfill
use acc_backfill::send_account_stream;

use {
    solana_client::nonblocking::rpc_client::RpcClient,
    solana_commitment_config::CommitmentConfig,
    solana_sdk::pubkey::Pubkey,
    std::str::FromStr,
};

/// Backfill specific mints by their addresses
/// This is useful for backfilling Token-2022 mints that were missed or need reindexing
pub async fn backfill_mints(rpc_url: String, redis_url: String, mints: Vec<String>) {
    info!("Starting mint backfill for {} mints", mints.len());

    // Parse mint pubkeys
    let mint_pubkeys: Vec<Pubkey> = mints
        .iter()
        .filter_map(|mint_str| {
            Pubkey::from_str(mint_str)
                .map_err(|e| {
                    error!("Failed to parse mint pubkey {}: {:?}", mint_str, e);
                    e
                })
                .ok()
        })
        .collect();

    if mint_pubkeys.is_empty() {
        error!("No valid mint pubkeys provided");
        return;
    }

    info!("Successfully parsed {} mint pubkeys", mint_pubkeys.len());

    let account_stream = async_stream::stream! {
        let client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::finalized());

        // Fetch current slot
        let slot = match client.get_slot().await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get current slot: {:?}", e);
                0
            }
        };

        info!("Fetching accounts at slot {}", slot);

        // Fetch all mint accounts
        let mut batch = Vec::new();
        for (idx, mint_pubkey) in mint_pubkeys.iter().enumerate() {
            info!("Fetching mint {}/{}: {}", idx + 1, mint_pubkeys.len(), mint_pubkey);

            match client.get_account(mint_pubkey).await {
                Ok(account) => {
                    info!("Successfully fetched mint {}: {} bytes", mint_pubkey, account.data.len());
                    batch.push((Some(account), *mint_pubkey));
                }
                Err(e) => {
                    error!("Failed to fetch mint {}: {:?}", mint_pubkey, e);
                    // Still add None to batch to indicate missing account
                    batch.push((None, *mint_pubkey));
                }
            }
        }

        if !batch.is_empty() {
            info!("Yielding batch of {} accounts", batch.len());
            yield (batch, slot);
        }
    };

    // Use the shared send_account_stream from acc_backfill
    send_account_stream(account_stream, redis_url).await;
    info!("Completed mint backfill");
}
