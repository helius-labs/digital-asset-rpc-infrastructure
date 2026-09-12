use crate::{error::DasJobErr, jobs::util::forward_mint, metric};
use cadence_macros::{is_global_default_set, statsd_count};
use digital_asset_types::dao::{asset, sea_orm_active_enums::OwnerType};
use log::{error, info};
use plerkle_messenger::Messenger;
use sea_orm::{entity::*, query::*, ColumnTrait, DatabaseConnection};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use tokio::{sync::Mutex, time::Instant};

const PAGE_SIZE: u64 = 100;

/// This job is used to fix assets that shows NULL owner in the database, but actually have an owner on chain.
pub async fn fix_missing_owner(
    conn: &DatabaseConnection,
    client: &RpcClient,
    messenger: &Arc<Mutex<Box<dyn Messenger>>>,
) -> Result<(), DasJobErr> {
    let mut assets = asset::Entity::find()
        .filter(asset::Column::Compressed.eq(false)) // only fix regular NFTs
        .filter(asset::Column::OwnerType.eq(OwnerType::Single))
        .filter(asset::Column::Owner.is_null())
        .filter(asset::Column::Supply.eq(1))
        .filter(asset::Column::Burnt.eq(false))
        .paginate(conn, PAGE_SIZE);

    let mut assets_found = 0;
    let mut errors = 0;
    let start_time = Instant::now(); // Record the start time
    while let Some(assets) = assets.fetch_and_next().await? {
        assets_found += assets.len();
        for asset in assets {
            let id = bs58::encode(asset.id.to_owned()).into_string();
            info!("Forwarding mint account: {}", id);
            match forward_mint(id, client, messenger).await {
                Ok(_) => {}
                Err(e) => {
                    error!("Error forwarding mint: {:?}", e);
                    errors += 1;
                }
            }
        }
    }

    metric! {
        statsd_count!("das_job.fix_missing_owner.duration_ms", start_time.elapsed().as_millis() as i64);
    }
    metric! {
        statsd_count!("das_job.fix_missing_owner.assets_found", assets_found as i64);
    }
    metric! {
        statsd_count!("das_job.fix_missing_owner.errors", errors);
    }

    Ok(())
}
