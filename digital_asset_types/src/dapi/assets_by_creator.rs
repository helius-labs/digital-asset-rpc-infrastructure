use crate::dao::scopes;
use crate::dao::scopes::asset::add_collection_metadata;
use crate::rpc::filter::AssetSorting;
use crate::rpc::response::AssetList;
use crate::rpc::DisplayOptions;

use crate::rpc::transform::AssetTransform;
use sea_orm::DatabaseConnection;
use sea_orm::DbErr;

use super::common::{build_asset_response, create_pagination, create_sorting};

pub async fn get_assets_by_creator(
    db: &DatabaseConnection,
    creator: Vec<u8>,
    only_verified: bool,
    sorting: AssetSorting,
    limit: u64,
    page: Option<u64>,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    transform: &AssetTransform,
    enable_grand_total_query: bool,
    enable_collection_metadata: bool,
    display_options: Option<DisplayOptions>,
) -> Result<AssetList, DbErr> {
    let pagination = create_pagination(before, after, page)?;
    let (sort_direction, sort_column) = create_sorting(sorting);

    let enable_grand_total_query = display_options.as_ref().map_or(false, |options| {
        enable_grand_total_query && options.show_grand_total
    });

    let (assets, grand_total) = scopes::asset::get_by_creator(
        db,
        creator,
        only_verified,
        sort_column,
        sort_direction,
        &pagination,
        limit,
        enable_grand_total_query,
    )
    .await?;

    let mut asset_list = build_asset_response(assets, limit, grand_total, &pagination, &transform);
    if let Some(display_options) = &display_options {
        if display_options.show_collection_metadata && enable_collection_metadata {
            asset_list = add_collection_metadata(db, asset_list).await?;
        }
    }

    Ok(asset_list)
}
