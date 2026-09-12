use super::common::build_asset_response;
use super::common::{create_pagination, create_sorting};
use crate::dao::scopes::asset::add_collection_metadata;
use crate::dao::{scopes, PageOptions};
use crate::dapi::last_indexed_slot::load_last_indexed_slot;
use crate::feature_flag::FeatureFlags;
use crate::rpc::filter::AssetSorting;
use crate::rpc::options::Options;
use crate::rpc::response::AssetList;
use sea_orm::DatabaseConnection;
use sea_orm::DbErr;

pub async fn get_assets_by_authority(
    db: &DatabaseConnection,
    authority: Vec<u8>,
    sorting: AssetSorting,
    page_options: &PageOptions,
    feature_flags: &FeatureFlags,
    options: &Options,
) -> Result<AssetList, DbErr> {
    let last_indexed_slot = load_last_indexed_slot(db).await?;
    let pagination = create_pagination(&page_options)?;
    let (sort_direction, sort_column) = create_sorting(sorting);

    let enable_grand_total_query =
        feature_flags.enable_grand_total_query && options.show_grand_total;

    let (assets, grand_total) = scopes::asset::get_by_authority(
        db,
        authority,
        sort_column,
        sort_direction,
        &pagination,
        page_options.limit,
        enable_grand_total_query,
        options,
    )
    .await?;

    let mut asset_list = build_asset_response(
        last_indexed_slot,
        assets,
        page_options.limit,
        grand_total,
        &pagination,
        options,
    );
    if options.show_collection_metadata && feature_flags.enable_collection_metadata {
        add_collection_metadata(db, &mut asset_list.items).await?;
    }

    Ok(asset_list)
}
