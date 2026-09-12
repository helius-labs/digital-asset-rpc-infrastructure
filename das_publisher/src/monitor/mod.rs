use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use cadence_macros::statsd_gauge;
use common::metric;
use digital_asset_types::dao::blocks;
use log::info;
use once_cell::sync::Lazy;
use sea_orm::{sea_query::Expr, DatabaseConnection, EntityTrait, FromQueryResult, QuerySelect};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use tokio::{
    task::JoinHandle,
    time::{interval, sleep},
};

pub const HEALTH_CHECK_SLOT_DISTANCE: u64 = 20;

pub static LATEST_SLOT: Lazy<Arc<AtomicU64>> = Lazy::new(|| Arc::new(AtomicU64::new(0)));

pub async fn fetch_current_slot_with_infinite_retry(client: &RpcClient) -> u64 {
    loop {
        match client
            .get_slot_with_commitment(CommitmentConfig::confirmed())
            .await
        {
            Ok(slot) => {
                return slot;
            }
            Err(e) => {
                log::error!("Failed to fetch current slot: {}", e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

pub async fn update_latest_slot(rpc_client: &RpcClient) {
    let slot = fetch_current_slot_with_infinite_retry(rpc_client).await;
    LATEST_SLOT.fetch_max(slot, Ordering::SeqCst);
}

pub async fn start_latest_slot_updater(rpc_client: Arc<RpcClient>) {
    if LATEST_SLOT.load(Ordering::SeqCst) != 0 {
        return;
    }
    update_latest_slot(&rpc_client).await;
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            update_latest_slot(&rpc_client).await;
        }
    });
}

pub fn continously_monitor_das(
    db: DatabaseConnection,
    rpc_client: Arc<RpcClient>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        start_latest_slot_updater(rpc_client.clone()).await;

        loop {
            let latest_slot = LATEST_SLOT.load(Ordering::SeqCst);
            let last_indexed_slot = fetch_last_indexed_slot_with_infinite_retry(&db).await;
            let lag = if latest_slot > last_indexed_slot {
                latest_slot - last_indexed_slot
            } else {
                0
            };
            metric! {
                statsd_gauge!("indexing_lag", lag);
            }
            info!("Indexing lag: {}", lag);
            sleep(Duration::from_secs(5)).await;
        }
    })
}

#[derive(FromQueryResult)]
pub struct OptionalSlotModel {
    pub slot: Option<i64>,
}

pub async fn fetch_last_indexed_optional_slot_with_infinite_retry(
    db_conn: &DatabaseConnection,
) -> Option<u64> {
    loop {
        let context = blocks::Entity::find()
            .select_only()
            .column_as(Expr::col(blocks::Column::Slot).max(), "slot")
            .into_model::<OptionalSlotModel>()
            .one(db_conn)
            .await;

        match context {
            Ok(context) => {
                return context
                    .expect("Always expected maximum query to return a result")
                    .slot
                    .map(|slot| slot as u64);
            }
            Err(e) => {
                log::error!("Failed to fetch current slot from database: {}", e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

pub async fn fetch_last_indexed_slot_with_infinite_retry(db_conn: &DatabaseConnection) -> u64 {
    loop {
        let slot = fetch_last_indexed_optional_slot_with_infinite_retry(db_conn).await;
        if let Some(slot) = slot {
            return slot;
        }
        sleep(Duration::from_millis(100)).await;
    }
}
