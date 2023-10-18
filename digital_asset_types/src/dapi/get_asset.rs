use std::collections::HashMap;

use super::common::{asset_to_rpc, build_asset_response};
use crate::{
    dao::{
        scopes::{self, asset::add_collection_metadata},
        Pagination,
    },
    feature_flag::FeatureFlags,
    rpc::{display_options::DisplayOptions, Asset},
};
use sea_orm::{DatabaseConnection, DbErr};

pub async fn get_asset(
    db: &DatabaseConnection,
    id: Vec<u8>,
    feature_flags: &FeatureFlags,
    display_options: &DisplayOptions,
) -> Result<Asset, DbErr> {
    let asset = scopes::asset::get_by_id(db, id, false, display_options).await?;
    let mut asset = asset_to_rpc(asset, display_options)?;
    if display_options.show_collection_metadata && feature_flags.enable_collection_metadata {
        let mut v = vec![asset.clone()];
        add_collection_metadata(db, &mut v).await?;
        asset = v.pop().unwrap_or(asset);
    }
    return Ok(asset);
}

pub async fn get_asset_batch(
    db: &DatabaseConnection,
    ids: Vec<Vec<u8>>,
    limit: u64,
    display_options: &DisplayOptions,
) -> Result<HashMap<String, Asset>, DbErr> {
    let pagination = Pagination::Page { page: 1 };
    let assets = scopes::asset::get_asset_batch(db, ids, &pagination, limit).await?;
    let mut asset_list = build_asset_response(assets, limit, None, &pagination, display_options);
    if display_options.show_collection_metadata {
        add_collection_metadata(db, &mut asset_list.items).await?;
    }
    let asset_map = asset_list
        .items
        .into_iter()
        .map(|asset| (asset.id.clone(), asset))
        .collect();
    Ok(asset_map)
}
