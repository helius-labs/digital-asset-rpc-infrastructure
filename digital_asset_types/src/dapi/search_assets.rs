use super::{
    common::{build_asset_response, create_pagination, create_sorting},
    get_assets_by_owner,
};
use crate::{
    dao::{
        scopes::{self, asset::add_collection_metadata},
        PageOptions, SearchAssetsQuery,
    },
    dapi::last_indexed_slot::load_last_indexed_slot,
    feature_flag::FeatureFlags,
    rpc::{filter::AssetSorting, options::Options, response::AssetList},
};
use sea_orm::{Condition, DatabaseConnection, DbErr, RelationDef};

pub async fn search_tokens(
    db: &DatabaseConnection,
    search_assets_query: SearchAssetsQuery,
    sorting: AssetSorting,
    page_options: &PageOptions,
    feature_flags: &FeatureFlags,
    options: &Options,
) -> Result<AssetList, DbErr> {
    let mut collection_id = None;
    if let Some(g) = search_assets_query.grouping {
        collection_id = Some(g.1)
    }
    return get_assets_by_owner(
        db,
        search_assets_query.owner_address.unwrap_or_default(),
        sorting,
        page_options,
        feature_flags,
        options,
        search_assets_query.token_type,
        collection_id,
        search_assets_query.supply_mint,
    )
    .await;
}

pub async fn search_assets(
    db: &DatabaseConnection,
    search_assets_query: SearchAssetsQuery,
    sorting: AssetSorting,
    page_options: &PageOptions,
    feature_flags: &FeatureFlags,
    options: &Options,
) -> Result<AssetList, DbErr> {
    let last_indexed_slot = load_last_indexed_slot(db).await?;
    let pagination = create_pagination(&page_options)?;
    let condition: Condition;
    let joins: Vec<RelationDef>;
    let (sort_direction, sort_column) = create_sorting(sorting);
    let (c, j) = search_assets_query.conditions()?;
    condition = c;
    joins = j;

    let enable_grand_total_query =
        feature_flags.enable_grand_total_query && options.show_grand_total;

    let (assets, grand_total) = scopes::asset::get_assets_by_condition(
        db,
        condition,
        joins,
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
