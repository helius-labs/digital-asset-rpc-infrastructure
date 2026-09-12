use crate::error::DasJobErr;
use crate::jobs::util::{
    get_account_closure_status, get_aggressive_retry_strategy, parse_checkpoint,
};
use crate::metric;
use async_stream::stream;
#[allow(unused_imports)]
use cadence_macros::is_global_default_set;
use cadence_macros::statsd_count;
use digital_asset_types::dao::asset;
use digital_asset_types::dao::sea_orm_active_enums::OwnerType;
use futures::pin_mut;
use futures::Stream;
use futures::StreamExt;
use log::{error, info};
use mpl_token_metadata::accounts::Metadata;
use nft_ingester::program_transformers::account_closure::mark_nft_as_burnt;
#[allow(unused_imports)]
use sea_orm::sea_query::OnConflict;
use sea_orm::ColumnTrait;
use sea_orm::DatabaseConnection;
#[allow(unused_imports)]
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryOrder;
use sea_orm::QuerySelect;
#[allow(unused_imports)]
use sea_orm::{ConnectionTrait, Order};
#[allow(unused_imports)]
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use sqlx::types::chrono;
use std::sync::Arc;
use std::time::Duration;
use tokio_retry::Retry;

use futures::stream::TryStreamExt;

pub async fn handle_nft_burns_job(
    conn: Arc<DatabaseConnection>,
    client: Arc<RpcClient>,
    max_concurrent_calls: usize,
    checkpoint: Option<String>,
) -> Result<(), DasJobErr> {
    info!("Starting handle_nft_burns_job");

    let checkpoint = parse_checkpoint(checkpoint)?;
    let stream = get_nft_stream(conn.clone(), checkpoint);
    // Needed because stream futures can refer to the stream. So we need to pin the stream to
    // make sure it doesn't move in memory. If not, the code won't compile.
    pin_mut!(stream);
    stream
        .map(|result| async {
            match result {
                Ok(token_account) => {
                    let strategy = get_aggressive_retry_strategy();
                    Retry::spawn(strategy, || async {
                        mark_nft_as_burnt_if_burnt(
                            client.clone(),
                            conn.clone(),
                            token_account.clone(),
                        )
                        .await
                    })
                    .await
                }
                Err(e) => {
                    error!("Error fetching NFT: {:?}", e);
                    Err(e)
                }
            }
        })
        .buffer_unordered(max_concurrent_calls)
        .try_collect::<()>()
        .await
}

fn get_nft_stream(
    conn: Arc<DatabaseConnection>,
    checkpoint: Option<Vec<u8>>,
) -> impl Stream<Item = Result<asset::Model, DasJobErr>> {
    let stream = stream! {
        let mut last_nft: Option<Vec<u8>> = checkpoint;
        let mut count = 0;
        loop {
            if let Some(nft) = last_nft.clone() {
                info!("Last NFT seen: {}", Pubkey::try_from(nft).expect("Unable to parse pubkey"));
            }

            let query = asset::Entity::find()
                .filter(asset::Column::Burnt.eq(false).or(asset::Column::Burnt.is_null())
                .and(asset::Column::MetadataAccountId.is_null())
                .and(asset::Column::Compressed.eq(false))
                .and(asset::Column::OwnerType.ne(OwnerType::Token))
                // Filter token accounts created less than 1 minute ago to avoid cluster sync issues.
                .and(asset::Column::CreatedAt.lt(chrono::Utc::now() - Duration::from_secs(60))))
                .order_by(asset::Column::Id, Order::Asc)
                .limit(1_000);

            let query = if let Some(nft) = last_nft.clone() {
                query.filter(asset::Column::Id.gt(nft))
            } else {
                query
            };
            info!("Fetching NFTs");
            match Retry::spawn(get_aggressive_retry_strategy(), || async {
                query.clone().all(&*conn).await
            }
            ).await {
                Ok(nfts) => {
                    count += nfts.len();
                    if nfts.is_empty() {
                        break;
                    }
                    last_nft = nfts.last().map(|nft| nft.id.clone());
                    for nft in nfts {
                        yield Ok::<asset::Model, DasJobErr>(nft);
                    }
                },
                Err(e) => {
                    yield Err(e.into());
                    break;
                }
            }
            info!("Total NFTs processed: {}", count);
        }
    };
    stream
}

async fn mark_nft_as_burnt_if_burnt(
    client: Arc<RpcClient>,
    conn: Arc<DatabaseConnection>,
    nft: asset::Model,
) -> Result<(), DasJobErr> {
    let metadata_account_id =
        Metadata::find_pda(&Pubkey::try_from(nft.id.clone()).expect("Unable to parse pubkey")).0;
    let res = match get_account_closure_status(client.clone(), metadata_account_id).await? {
        Some(slot) => mark_nft_as_burnt(nft, &conn, slot as u64)
            .await
            .map_err(|e| e.into()),
        None => Ok(()),
    };
    metric! {
        statsd_count!("nft_burn_job.nfts_processed", 1);
    }
    res
}
