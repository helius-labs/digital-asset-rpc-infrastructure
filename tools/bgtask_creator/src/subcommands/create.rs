use crate::{
    args::Args,
    utils::{find_by_type, find_daily_tasks, get_task_map},
};
use cadence_macros::{is_global_default_set, statsd_gauge};
use chrono::Utc;
use digital_asset_types::dao::offchain_metadata;
use futures::TryStreamExt;
use log::{debug, error, info};
use nft_ingester::{
    metric,
    tasks::{background_task_listener, hash_task, DownloadMetadata, IntoTaskData},
};
use sea_orm::{
    sea_query::Expr, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    SqlxPostgresConnector,
};
use solana_sdk::bs58;
use sqlx::{Pool, Postgres};
use std::thread::sleep;

pub async fn daily_job(conn: DatabaseConnection, database_pool: Pool<Postgres>, args: Args) -> () {
    let metadata = find_daily_tasks();

    let metrics_enabled = args.metrics_enabled.unwrap_or(false);

    let total_count = metadata.clone().0.count(&conn).await.unwrap();
    info!("Total matched: {}", total_count);

    if metrics_enabled {
        metric! {
            statsd_gauge!("offchain_data_missing.count", total_count);
        }
    }

    let mut metadata_missing = metadata
        .0
        .clone()
        .paginate(&conn, args.batch_size)
        .into_stream();

    let task_map = get_task_map();
    let mut count = 0;
    let mut tasks = vec![];
    while let Some(metadatas) = metadata_missing.try_next().await.unwrap() {
        info!("-- Processed: {}/{} --", count, total_count);

        for meta in metadatas {
            let asset_id = bs58::encode(meta.id.to_be_bytes()).into_string();
            let mut task = DownloadMetadata {
                asset_data_id: asset_id.clone().into(),
                uri: meta.metadata_url,
                created_at: Some(Utc::now().naive_utc()),
            };

            task.sanitize();
            let task_data = task.clone().into_task_data().unwrap();

            debug!(
                "Print task {} hash {:?}, uri: {:?}, asset_id: {:?}",
                task_data.data,
                hash_task(task_data.name.to_string(), task_data.data.clone()).unwrap(),
                task.uri,
                asset_id
            );

            if let Ok(hash) = hash_task(task_data.name.to_string(), task_data.data.clone()) {
                let database_pool = database_pool.clone();
                let task_map = task_map.clone();
                tasks.push(tokio::task::spawn(async move {
                    let res = background_task_listener::BackgroundTaskListener::save_new_task(
                        database_pool.clone(),
                        task_data,
                        task_map.clone(),
                    )
                    .await;

                    match res {
                        Ok(_) => debug!("Task created: {:?}. Asset: {:?}", hash, asset_id),
                        Err(e) => error!(
                            "Could not create task. Error: {:?}. Asset: {:?}",
                            e, asset_id
                        ),
                    }
                }));
            }
            count += 1;
        }
        if args.limit > 0 && count >= args.limit {
            break;
        }
    }

    if tasks.is_empty() {
        info!("No assets with missing metadata found");
    } else {
        let mut sent = 0;
        let mut failed = 0;
        for task in tasks {
            match task.await {
                Ok(_) => sent += 1,
                Err(e) => {
                    info!("Could not send task: {}", e);
                    failed += 1;
                }
            }
        }
        info!("Tasks sent={}, failed={}", sent, failed);
    }

    if metrics_enabled {
        // it can take a while for the tasks to be processed, so wait a bit before checking
        sleep(std::time::Duration::from_secs(20));

        let failed = metadata.0;
        let failed_count = failed.count(&conn).await.unwrap();
        let fixed_count = total_count.saturating_sub(failed_count);

        info!("Fixed count: {}", fixed_count);
        metric! {
            statsd_gauge!("offchain_data_fixed.count", fixed_count);
        }
    }
}

pub async fn create(conn: DatabaseConnection, database_pool: Pool<Postgres>, args: Args) -> () {
    if args.last_day.unwrap_or(false).clone() {
        return daily_job(conn, database_pool, args).await;
    }
    let asset_data = find_by_type(args.clone());

    let force_reindex = args.force_reindex.unwrap_or(false);

    let mut asset_data_missing = asset_data
        .0
        .clone()
        .paginate(&conn, args.batch_size)
        .into_stream();

    let task_map = get_task_map();
    let mut count = 0;
    let mut tasks = vec![];
    while let Some(assets) = asset_data_missing.try_next().await.unwrap() {
        info!("-- Processed: {}--", count);

        for asset in assets {
            let asset_id = bs58::encode(asset.id.clone()).into_string();
            let mut task = DownloadMetadata {
                asset_data_id: asset.id,
                uri: asset.metadata_url,
                created_at: Some(Utc::now().naive_utc()),
            };

            task.sanitize();
            let task_data = task.clone().into_task_data().unwrap();

            debug!(
                "Print task {} hash {:?}, uri: {:?}, asset_id: {:?}",
                task_data.data,
                hash_task(task_data.name.to_string(), task_data.data.clone()).unwrap(),
                task.uri,
                asset_id
            );

            if let Ok(hash) = hash_task(task_data.name.to_string(), task_data.data.clone()) {
                let database_pool = database_pool.clone();
                let task_map = task_map.clone();
                tasks.push(tokio::task::spawn(async move {
                    let conn =
                        SqlxPostgresConnector::from_sqlx_postgres_pool(database_pool.clone());

                    if force_reindex {
                        // make reindex=true so that they're not skipped by the should_reindex check
                        if let Err(e) = offchain_metadata::Entity::update_many()
                            .col_expr(offchain_metadata::Column::Reindex, Expr::value(true))
                            .filter(offchain_metadata::Column::MetadataUrl.eq(task.uri.clone()))
                            .exec(&conn)
                            .await
                        {
                            error!("Error updating reindex=true for asset {}: {}", asset_id, e);
                        }
                    }

                    let res = background_task_listener::BackgroundTaskListener::save_new_task(
                        database_pool.clone(),
                        task_data,
                        task_map.clone(),
                    )
                    .await;

                    match res {
                        Ok(_) => debug!("Task created: {:?}. Asset: {:?}", hash, asset_id),
                        Err(e) => error!(
                            "Could not create task. Error: {:?}. Asset: {:?}",
                            e, asset_id
                        ),
                    }
                }));
            }
            count += 1;
        }
        if args.limit > 0 && count >= args.limit {
            break;
        }
    }

    if tasks.is_empty() {
        info!("No assets with missing metadata found");
    } else {
        let mut sent = 0;
        let mut failed = 0;
        for task in tasks {
            match task.await {
                Ok(_) => sent += 1,
                Err(e) => {
                    info!("Could not send task: {}", e);
                    failed += 1;
                }
            }
        }
        info!("Tasks sent={}, failed={}", sent, failed);
    }
}
