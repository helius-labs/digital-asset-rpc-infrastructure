use crate::error::DasJobErr;
use crate::jobs::util::{
    get_account_closure_status, get_aggressive_retry_strategy, parse_checkpoint,
};
use crate::metric;
use async_stream::stream;
use cadence_macros::is_global_default_set;
use cadence_macros::statsd_count;
use digital_asset_types::dao::owners;
use futures::pin_mut;
use futures::Stream;
use futures::StreamExt;
use log::{error, info};
use nft_ingester::program_transformers::account_closure::mark_token_account_as_closed;
use sea_orm::ColumnTrait;
use sea_orm::DatabaseConnection;
use sea_orm::EntityTrait;
use sea_orm::Order;
use sea_orm::QueryFilter;
use sea_orm::QueryOrder;
use sea_orm::QuerySelect;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use sqlx::types::chrono;
use std::sync::Arc;
use std::time::Duration;
use tokio_retry::Retry;

use futures::stream::TryStreamExt;

pub async fn handle_missed_account_closures(
    conn: Arc<DatabaseConnection>,
    client: Arc<RpcClient>,
    max_concurrent_calls: usize,
    checkpoint: Option<String>,
    dryrun: bool,
) -> Result<(), DasJobErr> {
    info!("Starting handle_missed_account_closures job");
    let checkpoint = parse_checkpoint(checkpoint)?;
    let stream = get_token_account_stream(conn.clone(), checkpoint);
    // Needed because stream futures can refer to the stream. So we need to pin the stream to
    // make sure it doesn't move in memory. If not, the code won't compile.
    pin_mut!(stream);
    stream
        .map(|result| async {
            match result {
                Ok(token_account) => {
                    let strategy = get_aggressive_retry_strategy();
                    Retry::spawn(strategy, || async {
                        mark_token_account_as_closed_if_closed(
                            client.clone(),
                            conn.clone(),
                            token_account.clone(),
                            dryrun,
                        )
                        .await
                    })
                    .await
                }
                Err(e) => {
                    error!("Error fetching token account: {:?}", e);
                    Err(e)
                }
            }
        })
        .buffer_unordered(max_concurrent_calls)
        .try_collect::<()>()
        .await
}

fn get_token_account_stream(
    conn: Arc<DatabaseConnection>,
    checkpoint: Option<Vec<u8>>,
) -> impl Stream<Item = Result<owners::Model, DasJobErr>> {
    let stream = stream! {
        let mut last_token_account: Option<Vec<u8>> = checkpoint;
        let mut count = 0;
        loop {
            // TODO: Stop selecting all of the data. We just need the token account
            if let Some(token_account) = last_token_account.clone() {
                info!("Last token account seen: {}", Pubkey::try_from(token_account).expect("Unable to parse pubkey"));
            }
            let query = owners::Entity::find()
                .filter((owners::Column::Closed.eq(false).or(owners::Column::Closed.is_null()))
                .and(owners::Column::TokenAccount.is_not_null())
                // Filter token accounts created less than 1 minute ago to avoid cluster sync issues.
                .and(owners::Column::CreatedAt.lt(chrono::Utc::now() - Duration::from_secs(60))))
                .order_by(owners::Column::TokenAccount, Order::Asc)
                .limit(10_000);

            let query = if let Some(token_account) = last_token_account.clone() {
                query.filter(owners::Column::TokenAccount.gt(token_account))
            } else {
                query
            };
            info!("Fetching token accounts");
            match Retry::spawn(get_aggressive_retry_strategy(), || async {
                query.clone().all(&*conn).await
            }
            ).await {
                Ok(token_accounts) => {
                    count += token_accounts.len();
                    if token_accounts.is_empty() {
                        break;
                    }
                    last_token_account = token_accounts.last().map(|t| t.token_account.clone()).ok_or(DasJobErr::DbError("Unable to get last token account".to_string()))?;
                    for token_account in token_accounts {
                        let token_account: owners::Model = token_account;
                        yield Ok::<owners::Model, DasJobErr>(token_account);
                    }
                },
                Err(e) => {
                    yield Err(e.into());
                    break;
                }
            }
            info!("Total token accounts processed: {}", count);
        }
    };
    stream
}

async fn mark_token_account_as_closed_if_closed(
    client: Arc<RpcClient>,
    conn: Arc<DatabaseConnection>,
    token_account: owners::Model,
    dryrun: bool,
) -> Result<(), DasJobErr> {
    let id = token_account
        .token_account
        .clone()
        .ok_or(DasJobErr::DbError("Token account is null".to_string()))?;
    let id = Pubkey::try_from(id).expect("Unable to parse pubkey");
    let res = match get_account_closure_status(client.clone(), id).await? {
        Some(slot) => {
            info!("Token account is closed: {}", id);
            if dryrun {
                metric! {
                    statsd_count!("token_account.closed_dryrun", 1);
                }
                Ok(())
            } else {
                mark_token_account_as_closed(token_account, &conn, slot as u64)
                    .await
                    .map_err(|e| e.into())
            }
        }
        None => Ok(()),
    };
    metric! {
        statsd_count!("account_closures_job.token_account_processed", 1);
    }
    res
}
