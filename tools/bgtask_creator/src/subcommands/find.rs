use crate::args::Args;

use chrono::Utc;
use digital_asset_types::dao::{
    asset_data_v2, offchain_metadata, sea_orm_active_enums::TaskStatus, tasks,
};
use nft_ingester::tasks::{hash_task, DownloadMetadata, IntoTaskData};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

pub async fn find(conn: DatabaseConnection, args: Args) -> () {
    let asset_id = Pubkey::from_str(args.mint.unwrap().as_str()).unwrap();
    let asset_id_bytes = asset_id.clone().to_bytes().to_vec();

    let (asset_data, offchain) = asset_data_v2::Entity::find_by_id(asset_id_bytes.clone())
        .find_also_related(offchain_metadata::Entity)
        .one(&conn)
        .await
        .unwrap()
        .unwrap();

    match offchain {
        Some(offchain) => println!("off-chain data for asset: {:?}", offchain.metadata),
        None => println!("off-chain data for asset: None"),
    }

    let mut task = DownloadMetadata {
        asset_data_id: asset_id_bytes.clone(),
        uri: asset_data.metadata_url,
        created_at: Some(Utc::now().naive_utc()),
    };
    task.sanitize();
    let task_data = task.clone().into_task_data().unwrap();
    let hash = hash_task(task_data.name.to_string(), task_data.data).unwrap();
    let task_entry = tasks::Entity::find_by_id(hash)
        .filter(tasks::Column::Status.ne(TaskStatus::Pending))
        .one(&conn)
        .await;
    println!("task: {:?}", task_entry)
}
