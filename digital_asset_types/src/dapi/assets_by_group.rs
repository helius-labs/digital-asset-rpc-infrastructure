use crate::dao::scopes;
use crate::dao::PageOptions;
use crate::feature_flag::FeatureFlags;
use crate::rpc::display_options::DisplayOptions;
use crate::rpc::filter::AssetSorting;
use crate::rpc::response::AssetList;
use sea_orm::DatabaseConnection;
use sea_orm::DbErr;

use super::common::create_sorting;
use super::common::{build_asset_response, create_pagination};

pub async fn get_assets_by_group(
    db: &DatabaseConnection,
    group_value: String,
    sorting: AssetSorting,
    page_options: &PageOptions,
    feature_flags: &FeatureFlags,
    display_options: &DisplayOptions,
) -> Result<AssetList, DbErr> {
    // TODO: Explore further optimizing the unsorted query
    let pagination = create_pagination(&page_options)?;
    let (sort_direction, sort_column) = create_sorting(sorting);

    let enable_grand_total_query =
        feature_flags.enable_grand_total_query && display_options.show_grand_total;

    let (assets, grand_total) = scopes::asset::get_by_grouping(
        db,
        group_value.clone(),
        sort_column,
        sort_direction,
        &pagination,
        page_options.limit,
        enable_grand_total_query,
        display_options.show_unverified_collections,
    )
    .await?;

    Ok(build_asset_response(
        assets,
        page_options.limit,
        grand_total,
        &pagination,
        display_options,
    ))
}
