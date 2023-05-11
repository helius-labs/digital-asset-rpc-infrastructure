use crate::dao::scopes;
use crate::rpc::filter::AssetSorting;
use crate::rpc::response::AssetList;
use crate::rpc::transform;
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
) -> Result<AssetList, DbErr> {
    let pagination = create_pagination(before, after, page)?;
    let (sort_direction, sort_column) = create_sorting(sorting);
    let assets = scopes::asset::get_by_creator(
        db,
        creator,
        only_verified,
        sort_column,
        sort_direction,
        &pagination,
        limit,
    )
    .await?;
    Ok(build_asset_response(assets, limit, &pagination, transform))
}
