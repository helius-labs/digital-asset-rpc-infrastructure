use crate::error::DasJobErr;
use digital_asset_types::dao::{asset_data, asset_data_v2, offchain_metadata};
use log::error;
use sea_orm::{
    sea_query::OnConflict, ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QueryTrait, Set, SqlxPostgresConnector,
};
use sqlx::{Pool, Postgres};
use tokio::task::JoinHandle;

/*
 Not used anymore. Kept for reference.
*/

const BATCH_SIZE: u64 = 1000;
const START_SLOT: u64 = 0;
const END_SLOT: u64 = 229900000;

pub fn start_asset_data_migration_job(
    pool: Pool<Postgres>,
    start_slot: Option<u64>,
    end_slot: Option<u64>,
    batch_size: Option<u64>,
) -> JoinHandle<()> {
    let start_slot = start_slot.unwrap_or(START_SLOT);
    let end_slot = end_slot.unwrap_or(END_SLOT);
    let batch_size = batch_size.unwrap_or(BATCH_SIZE);
    tokio::spawn(async move {
        migrate_data(pool, start_slot, end_slot, batch_size)
            .await
            .unwrap();
    })
}

pub async fn migrate_data(
    pool: Pool<Postgres>,
    start_slot: u64,
    end_slot: u64,
    batch_size: u64,
) -> Result<(), DasJobErr> {
    let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
    let mut records = asset_data::Entity::find()
        .filter(asset_data::Column::SlotUpdated.gte(start_slot))
        .filter(asset_data::Column::SlotUpdated.lte(end_slot))
        .order_by_asc(asset_data::Column::SlotUpdated)
        .paginate(&conn, batch_size);

    error!(
        target: "start_migration",
        "-------- Migration Started. Start Slot: {}. End Slot: {}. Batch Size: {}. --------",
        start_slot, end_slot, batch_size
    );

    while let Some(records) = records.fetch_and_next().await? {
        let mut tasks = vec![];

        for record in records {
            let pool = pool.clone();
            tasks.push(tokio::spawn(async move {
                let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
                let offchain_metadata_model = offchain_metadata::ActiveModel {
                    metadata_url: Set(record.metadata_url.clone()),
                    metadata: Set(record.metadata),
                    mutability: Set(record.metadata_mutability),
                    reindex: Set(record.reindex.unwrap_or(true)),
                    created_at: Set(record.created_at.unwrap_or(Default::default())),
                    ..Default::default()
                };
                let offchain_metadata_query =
                    offchain_metadata::Entity::insert(offchain_metadata_model)
                        .on_conflict(
                            OnConflict::columns([offchain_metadata::Column::MetadataUrl])
                                .do_nothing()
                                .to_owned(),
                        )
                        .build(DbBackend::Postgres);

                let asset_data_v2_model = asset_data_v2::ActiveModel {
                    metadata_url: Set(record.metadata_url.clone()),
                    id: Set(record.id.clone()),
                    chain_mutability: Set(record.chain_data_mutability),
                    chain_data: Set(record.chain_data),
                    slot_updated: Set(record.slot_updated.clone()),
                    raw_name: Set(record.raw_name),
                    raw_symbol: Set(record.raw_symbol),
                };
                let asset_data_v2_query = asset_data_v2::Entity::insert(asset_data_v2_model)
                    .on_conflict(
                        OnConflict::columns([asset_data_v2::Column::Id])
                            .do_nothing()
                            .to_owned(),
                    )
                    .build(DbBackend::Postgres);

                let offchain_res = conn
                    .execute(offchain_metadata_query)
                    .await
                    .map_err(|db_err| DasJobErr::AssetDataMigrationError(db_err.to_string()));
                let ad_v2_res = conn
                    .execute(asset_data_v2_query)
                    .await
                    .map_err(|db_err| DasJobErr::AssetDataMigrationError(db_err.to_string()));

                match offchain_res {
                    Ok(_) => {}
                    Err(err) => {
                        error!(
                            target: "offchain_metadata",
                            "Error inserting offchain metadata: {}. Slot: {}. Metadata URL: {}.",
                            err, record.slot_updated, record.metadata_url
                        );
                    }
                }
                match ad_v2_res {
                    Ok(_) => {}
                    Err(err) => {
                        error!(
                            target: "asset_data_v2",
                            "Error inserting asset data v2: {}. Slot: {}. Asset ID: {}",
                            err, record.slot_updated, bs58::encode(record.id).into_string()
                        );
                    }
                }
            }))
        }

        for task in tasks {
            match task.await {
                Ok(_) => {}
                Err(err) => {
                    error!("Error inserting data: {}", err);
                }
            }
        }
    }

    error!(
        target: "end_migration",
        "-------- Migration Completed. Start Slot: {}. End Slot: {} --------",
        start_slot, end_slot
    );
    Ok(())
}
