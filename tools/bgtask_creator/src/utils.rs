use chrono::{Duration, Utc};
use digital_asset_types::dao::{asset_data_v2, extensions, offchain_metadata};
use log::info;
use nft_ingester::tasks::{BgTask, BgTaskConfig, DownloadMetadataTask};
use sea_orm::RelationTrait;
use sea_orm::{query::*, sea_query::SimpleExpr, ColumnTrait, EntityTrait};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::{collections::HashMap, sync::Arc, time};

use crate::args::Args;

pub fn find_daily_tasks<'a>() -> (sea_orm::Select<offchain_metadata::Entity>, String) {
    let condition =
        offchain_metadata::Column::Metadata.eq(JsonValue::String("processing".to_string()));
    let stmt = offchain_metadata::Entity::find().filter(Condition::all().add(condition));

    let mut conditions = Condition::all();
    conditions =
        conditions.add(offchain_metadata::Column::CreatedAt.gte(Utc::now() - Duration::days(60)));

    info!("Finding all metadata within 2 days {:?}", conditions);
    (
        stmt.filter(Condition::all().add(conditions)),
        "all".to_string(),
    )
}

pub fn find_by_type<'a>(args: Args) -> (sea_orm::Select<asset_data_v2::Entity>, String) {
    let mut stmt = asset_data_v2::Entity::find();
    stmt = stmt.join(
        JoinType::LeftJoin,
        asset_data_v2::Relation::OffchainMetadata.def(),
    );

    let mut conditions = Condition::all();

    if args.missing_only.unwrap_or(false) {
        // Repair mode (e.g. periodic per-collection job): only assets whose off-chain
        // metadata was NEVER successfully fetched — NOT the default "stale after 1 day"
        // set, which on a large collection would re-fetch nearly everything every run.
        // `updated_at IS NULL` is exactly the never-fetched set (a successful download
        // always stamps updated_at); the "processing" sentinel rows are a subset of it.
        conditions = conditions.add(offchain_metadata::Column::UpdatedAt.is_null());
    } else if !args.force_reindex.unwrap_or(false).clone() {
        let missing_cond = Condition::any()
            .add(offchain_metadata::Column::Reindex.eq(true))
            .add(offchain_metadata::Column::UpdatedAt.is_null())
            // we consider it stale after 1 day
            .add(offchain_metadata::Column::UpdatedAt.lt(Utc::now() - Duration::days(1)));
        conditions = conditions.add(missing_cond);
    }

    if args.last_day.unwrap_or(false).clone() {
        let one_day_ago = Utc::now() - Duration::days(1);
        conditions = conditions.add(offchain_metadata::Column::CreatedAt.gte(one_day_ago));
    }

    if let Some(url) = args.include_url {
        conditions = conditions.add(asset_data_v2::Column::MetadataUrl.like(url.as_str()));
    }
    if let Some(url) = args.ignore_url {
        conditions = conditions.add(asset_data_v2::Column::MetadataUrl.not_like(url.as_str()));
    }

    if let Some(authority) = args.authority {
        info!(
            "Find asset data for authority {} conditions {:?}",
            authority, conditions
        );

        // JOIN asset
        stmt = stmt.join(
            JoinType::InnerJoin,
            extensions::asset_data_v2::Relation::Asset.def(),
        );
        stmt = stmt.filter(conditions.add(SimpleExpr::Custom(format!(
            "authorities_info -> 'authority' = '{}'::jsonb",
            authority.as_str()
        ))));

        (stmt, authority.to_string())
    } else if let Some(collection) = args.collection {
        info!(
            "Finding asset_data for collection {}, conditions {:?}",
            collection, conditions
        );

        // JOIN asset
        stmt = stmt.join(
            JoinType::InnerJoin,
            extensions::asset_data_v2::Relation::Asset.def(),
        );
        stmt = stmt.filter(conditions.add(SimpleExpr::Custom(format!(
            "collections_info ->> 'collection_id' = '{}'",
            collection.as_str()
        ))));

        (stmt, collection.to_string())
    } else if let Some(creator) = args.creator {
        info!(
            "Finding assets for creator {} with conditions {:?}",
            creator, conditions
        );

        // TODO: Refactor this to use asset::creators_info
        let pubkey = Pubkey::from_str(creator.as_str()).unwrap();
        let _pubkey_bytes = pubkey.to_bytes().to_vec();

        (stmt, creator.to_string())
    } else if let Some(mint) = args.mint {
        info!(
            "Finding assets for mint {}, conditions {:?}",
            mint, conditions
        );

        let pubkey = Pubkey::from_str(mint.as_str()).unwrap();
        let pubkey_bytes = pubkey.to_bytes().to_vec();

        (
            asset_data_v2::Entity::find_by_id(pubkey_bytes),
            mint.to_string(),
        )
    } else {
        info!("Finding all assets with condition {:?}", conditions);
        (
            stmt.filter(Condition::all().add(conditions)),
            "all".to_string(),
        )
    }
}

pub fn get_task_map() -> Arc<HashMap<String, Box<dyn BgTask>>> {
    let task_runner_config = BgTaskConfig::default();
    let bg_task_definitions: Vec<Box<dyn BgTask>> = vec![Box::new(DownloadMetadataTask {
        lock_duration: task_runner_config.lock_duration,
        max_attempts: task_runner_config.max_attempts,
        timeout: Some(time::Duration::from_secs(
            task_runner_config.timeout.unwrap_or(3),
        )),
    })];
    let mut bg_tasks = HashMap::new();
    for task in bg_task_definitions {
        bg_tasks.insert(task.name().to_string(), task);
    }
    Arc::new(bg_tasks)
}
