use log::{error, info};

// Import shared utilities from acc_backfill
use acc_backfill::send_account_stream;

use {
    anyhow::Context,
    solana_sdk::{account::Account, pubkey::Pubkey},
    std::{str::FromStr, time::Duration},
};

#[derive(Debug, serde::Deserialize)]
struct GetProgramAccountsV2Response {
    accounts: Vec<ProgramAccount>,
    #[serde(rename = "paginationKey")]
    pagination_key: Option<String>,
    #[allow(unused)]
    #[serde(rename = "totalResults")]
    total_results: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
struct ProgramAccount {
    pubkey: String,
    account: AccountData,
}

#[derive(Debug, serde::Deserialize)]
struct AccountData {
    data: Vec<String>,
    lamports: u64,
    owner: String,
    executable: bool,
    #[serde(rename = "rentEpoch")]
    rent_epoch: u64,
}

#[derive(Debug, serde::Deserialize)]
struct RpcJsonResponse {
    result: GetProgramAccountsV2Response,
}

/// Fetch program accounts with pagination using getProgramAccountsV2
async fn fetch_program_accounts_paginated(
    rpc_url: String,
    program_id: String,
    pagination_key: Option<String>,
    page_size: usize,
) -> anyhow::Result<(Vec<(Pubkey, Account)>, Option<String>, usize, u64)> {
    let client = reqwest::Client::new();

    // Build params with optional paginationKey
    let mut params_obj = serde_json::json!({
        "encoding": "base64",
        "commitment": "finalized",
        "limit": page_size
    });

    // Add paginationKey if present (for subsequent pages)
    if let Some(key) = pagination_key {
        params_obj["paginationKey"] = serde_json::json!(key);
    }

    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getProgramAccountsV2",
        "params": [program_id, params_obj]
    });

    let response = client
        .post(&rpc_url)
        .json(&request_body)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .context("Failed to send request")?;

    let response_text = response.text().await.context("Failed to read response")?;
    let parsed: RpcJsonResponse = serde_json::from_str(&response_text)
        .context(format!("Failed to parse response: {}", response_text))?;

    let result = parsed.result;
    let pagination_key = result.pagination_key;
    let total_results = result.accounts.len();
    let mut accounts = Vec::new();

    // Get current slot to represent when these accounts were fetched (like acc_backfill does)
    let slot = get_current_slot(&rpc_url).await.unwrap_or(0);

    for program_account in result.accounts {
        let pubkey = Pubkey::from_str(&program_account.pubkey)
            .context(format!("Failed to parse pubkey: {}", program_account.pubkey))?;

        // Decode base64 account data
        let account_data = if !program_account.account.data.is_empty() {
            #[allow(deprecated)]
            base64::decode(&program_account.account.data[0])
                .context(format!("Failed to decode account data for {}", pubkey))?
        } else {
            Vec::new()
        };

        let owner = Pubkey::from_str(&program_account.account.owner)
            .context(format!("Failed to parse owner: {}", program_account.account.owner))?;

        let account = Account {
            lamports: program_account.account.lamports,
            data: account_data,
            owner,
            executable: program_account.account.executable,
            rent_epoch: program_account.account.rent_epoch,
        };

        accounts.push((pubkey, account));
    }

    Ok((accounts, pagination_key, total_results, slot))
}

/// Get current slot from RPC (similar to how acc_backfill gets slot from RPC response)
async fn get_current_slot(rpc_url: &str) -> anyhow::Result<u64> {
    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSlot",
        "params": []
    });

    let response = client
        .post(rpc_url)
        .json(&request_body)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    #[derive(serde::Deserialize)]
    struct SlotResponse {
        result: u64,
    }

    let slot_response: SlotResponse = response.json().await?;
    Ok(slot_response.result)
}

/// Generic backfill function for any Solana program
/// Backfills all accounts owned by the specified program to Redis
pub async fn backfill_program_accounts(
    rpc_url: String,
    redis_url: String,
    program_id: String,
) {
    const PAGE_SIZE: usize = 1000;

    info!("Starting backfill for program {}", program_id);
    
    let program_id_clone = program_id.clone();
    let account_stream = async_stream::stream! {
        let mut pagination_key: Option<String> = None;
        let mut page_num = 1;
        let mut total_accounts = 0;
        let mut total_filtered = 0;

        loop {
            info!("Fetching page {} (pagination key: {})...", page_num, pagination_key.as_ref().unwrap_or(&"initial".to_string()));

            match fetch_program_accounts_paginated(
                rpc_url.clone(),
                program_id.clone(),
                pagination_key.clone(),
                PAGE_SIZE,
            ).await {
                Ok((accounts, next_pagination_key, results_count, slot)) => {
                    total_accounts += results_count;
                    let filtered_count = accounts.len();
                    total_filtered += filtered_count;

                    info!(
                        "Page {}: Got {} results, {} passed filter (total: {} accounts, {} filtered)",
                        page_num, results_count, filtered_count, total_accounts, total_filtered
                    );

                    if !accounts.is_empty() {
                        // Transform to acc_backfill's stream format: (Vec<(Option<Account>, Pubkey)>, slot)
                        let batch: Vec<(Option<Account>, Pubkey)> = accounts
                            .into_iter()
                            .map(|(pubkey, account)| (Some(account), pubkey))
                            .collect();

                        yield (batch, slot);
                    }

                    // Check if we have more pages (pagination_key is None when done)
                    if next_pagination_key.is_none() {
                        info!("Reached last page (no more pagination key). Total accounts: {}, Total filtered: {}",
                              total_accounts, total_filtered);
                        break;
                    }

                    pagination_key = next_pagination_key;
                    page_num += 1;
                }
                Err(e) => {
                    error!("Error fetching page {}: {:?}", page_num, e);
                    info!("Retrying page {} in 5 seconds...", page_num);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    };

    // Use the shared send_account_stream from acc_backfill
    send_account_stream(account_stream, redis_url).await;
    info!("Completed backfill for program {}", program_id_clone);
}
