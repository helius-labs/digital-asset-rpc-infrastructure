use super::common::{asset_to_rpc, build_asset_response};
use crate::{
    dao::{
        scopes::{
            self,
            asset::{add_collection_metadata, get_by_id},
        },
        Pagination,
    },
    dapi::last_indexed_slot::load_last_indexed_slot,
    feature_flag::FeatureFlags,
    rpc::{options::Options, response::AssetList, Asset},
};
use sea_orm::{DatabaseConnection, DbErr};

pub async fn get_asset(
    db: &DatabaseConnection,
    id: Vec<u8>,
    feature_flags: &FeatureFlags,
    options: &Options,
) -> Result<Asset, DbErr> {
    let last_indexed_slot = load_last_indexed_slot(db).await?;
    let asset = get_by_id(db, id, true, options, feature_flags.verify_asset_exists).await?;
    let mut asset = asset_to_rpc(Some(last_indexed_slot), asset, options)?;
    if options.show_collection_metadata && feature_flags.enable_collection_metadata {
        let mut v = vec![asset.clone()];
        add_collection_metadata(db, &mut v).await?;
        asset = v.pop().unwrap_or(asset);
    }
    Ok(asset)
}

pub async fn get_asset_list(
    db: &DatabaseConnection,
    ids: Vec<Vec<u8>>,
    limit: u64,
    feature_flags: &FeatureFlags,
    options: &Options,
) -> Result<AssetList, DbErr> {
    let last_indexed_slot = load_last_indexed_slot(db).await?;
    let pagination = Pagination::Page { page: 1 };
    let assets = scopes::asset::get_assets(db, ids, &pagination, limit, options).await?;
    let mut asset_list =
        build_asset_response(last_indexed_slot, assets, limit, None, &pagination, options);
    if options.show_collection_metadata && feature_flags.enable_collection_metadata {
        add_collection_metadata(db, &mut asset_list.items).await?;
    }
    Ok(asset_list)
}
