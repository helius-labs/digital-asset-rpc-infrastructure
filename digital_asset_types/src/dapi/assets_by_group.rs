use crate::dao::scopes;
use crate::rpc::filter::AssetSorting;
use crate::rpc::response::AssetList;

use crate::rpc::transform::AssetTransform;
use sea_orm::DatabaseConnection;
use sea_orm::DbErr;

use super::common::create_sorting;
use super::common::{build_asset_response, create_pagination};
pub async fn get_assets_by_group(
    db: &DatabaseConnection,
    group_key: String,
    group_value: String,
    sorting: AssetSorting,
    limit: u64,
    page: Option<u64>,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    transform: &AssetTransform,
    enable_grand_total_query: bool,
) -> Result<AssetList, DbErr> {
    // TODO: Explore further optimizing the unsorted query
    let pagination = create_pagination(before, after, page)?;
    let (sort_direction, sort_column) = create_sorting(sorting);
    let (assets, grand_total) = scopes::asset::get_by_grouping(
        db,
        group_key.clone(),
        group_value.clone(),
        sort_column,
        sort_direction,
        &pagination,
        limit,
        enable_grand_total_query,
    )
    .await?;

    Ok(build_asset_response(
        assets,
        limit,
        grand_total,
        &pagination,
        transform,
    ))
}
