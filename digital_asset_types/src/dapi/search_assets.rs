use super::common::{build_asset_response, create_pagination, create_sorting};
use crate::{
    dao::{
        scopes::{self, asset::add_collection_metadata},
        PageOptions, SearchAssetsQuery,
    },
    feature_flag::FeatureFlags,
    rpc::{display_options::DisplayOptions, filter::AssetSorting, response::AssetList},
};
use sea_orm::{DatabaseConnection, DbErr};

pub async fn search_assets(
    db: &DatabaseConnection,
    search_assets_query: SearchAssetsQuery,
    sorting: AssetSorting,
    page_options: &PageOptions,
    feature_flags: &FeatureFlags,
    display_options: &DisplayOptions,
) -> Result<AssetList, DbErr> {
    let pagination = create_pagination(&page_options)?;
    let (sort_direction, sort_column) = create_sorting(sorting);
    let (condition, joins) = search_assets_query.conditions()?;

    let enable_grand_total_query =
        feature_flags.enable_grand_total_query && display_options.show_grand_total;

    let (assets, grand_total) = scopes::asset::get_assets_by_condition(
        db,
        condition,
        joins,
        sort_column,
        sort_direction,
        &pagination,
        page_options.limit,
        enable_grand_total_query,
        display_options.show_unverified_collections,
    )
    .await?;
    let mut asset_list = build_asset_response(
        assets,
        page_options.limit,
        grand_total,
        &pagination,
        display_options,
    );
    if display_options.show_collection_metadata {
        add_collection_metadata(db, &mut asset_list.items).await?;
    }
    Ok(asset_list)
}
