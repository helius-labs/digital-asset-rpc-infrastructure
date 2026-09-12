use crate::{
    error::DasJobErr,
    jobs::util::{reindex_assets_concurrency, statsd_count_w_tag},
};
use digital_asset_types::{
    dao::{
        scopes::asset::{get_by_creator, get_by_grouping},
        PageOptions,
    },
    dapi::common::{create_pagination, create_sorting},
    rpc::{filter::AssetSorting, options::Options},
};
use log::info;
use plerkle_messenger::Messenger;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio::{sync::Mutex, time::Instant};

const MAX_CONCURRENCY: usize = 10;

pub async fn reindex_collection(
    conn: Arc<DatabaseConnection>,
    collection: &String,
    rpc_url: &String,
    messenger: &Arc<Mutex<Box<dyn Messenger>>>,
) -> Result<(), DasJobErr> {
    let start_time = Instant::now();
    let limit = 1000;
    let mut page = 0;

    loop {
        info!("Reindexing collection: {}, page {}", collection, page);
        let pagination_options = PageOptions {
            limit,
            page: Some(page),
            before: None,
            after: None,
            cursor: None,
        };
        let pagination = create_pagination(&pagination_options)?;
        let (sort_direction, sort_column) = create_sorting(AssetSorting::default());

        let (assets, _) = get_by_grouping(
            conn.as_ref(),
            "collection".to_string(),
            collection.clone(),
            sort_column,
            sort_direction,
            &pagination,
            limit,
            false,
            &Options::default(),
        )
        .await?;

        if assets.is_empty() {
            break;
        } else {
            page += 1;
        }

        reindex_assets_concurrency(
            conn.clone(),
            rpc_url,
            messenger,
            assets,
            "collection".to_string(),
            collection.to_string(),
            MAX_CONCURRENCY,
        )
        .await?;
    }

    info!("Done reindexing collection: {}", collection);
    statsd_count_w_tag(
        "das_job.reindex_collection.duration_ms",
        start_time.elapsed().as_millis() as i64,
        "collection",
        collection,
    );
    Ok(())
}

pub async fn reindex_creator(
    conn: Arc<DatabaseConnection>,
    creator: &String,
    rpc_url: &String,
    messenger: &Arc<Mutex<Box<dyn Messenger>>>,
) -> Result<(), DasJobErr> {
    let start_time = Instant::now();
    let limit = 1000;
    let mut page = 0;

    let creator_bytes = bs58::decode(creator)
        .into_vec()
        .map_err(|_| DasJobErr::ReindexError(format!("Invalid creator provided: {}", creator)))?;

    loop {
        info!("Reindexing creator: {}, page {}", creator, page);
        let pagination_options = PageOptions {
            limit,
            page: Some(page),
            before: None,
            after: None,
            cursor: None,
        };
        let pagination = create_pagination(&pagination_options)?;
        let (sort_direction, sort_column) = create_sorting(AssetSorting::default());

        let only_verified = true;
        let (assets, _) = get_by_creator(
            conn.as_ref(),
            creator_bytes.clone(),
            only_verified,
            sort_column,
            sort_direction,
            &pagination,
            limit,
            false,
            &Options::default(),
        )
        .await?;

        if assets.is_empty() {
            break;
        } else {
            page += 1;
        }

        for batch in assets.chunks(MAX_CONCURRENCY) {
            reindex_assets_concurrency(
                conn.clone(),
                rpc_url,
                messenger,
                batch.to_vec(),
                "creator".to_string(),
                creator.to_string(),
                MAX_CONCURRENCY,
            )
            .await?;
        }
    }

    info!("Done reindexing creator: {}", creator);
    statsd_count_w_tag(
        "das_job.reindex_creator.duration_ms",
        start_time.elapsed().as_millis() as i64,
        "creator",
        creator,
    );
    Ok(())
}
