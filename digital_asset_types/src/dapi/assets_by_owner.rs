use super::common::create_owner_sorting;
use super::common::{build_asset_response, create_pagination, create_sorting};
use crate::dao::scopes;
use crate::dao::scopes::asset::{add_collection_metadata, TokenType};
use crate::dao::PageOptions;
use crate::dapi::last_indexed_slot::load_last_indexed_slot;
use crate::feature_flag::FeatureFlags;
use crate::rpc::filter::AssetSorting;
use crate::rpc::options::Options;
use crate::rpc::response::AssetList;
use sea_orm::DatabaseConnection;
use sea_orm::DbErr;

pub async fn get_assets_by_owner(
    db: &DatabaseConnection,
    owner_address: Vec<u8>,
    sort_by: AssetSorting,
    page_options: &PageOptions,
    feature_flags: &FeatureFlags,
    options: &Options,
    token_type: Option<TokenType>,
    collection_id: Option<String>,
    asset_id: Option<Vec<u8>>,
) -> Result<AssetList, DbErr> {
    let last_indexed_slot = load_last_indexed_slot(db).await?;
    let pagination = create_pagination(&page_options)?;

    let enable_grand_total_query =
        feature_flags.enable_grand_total_query && options.show_grand_total;

    // When show_fungible=false (default) and no specific token_type requested,
    // use the NFT-only path that queries asset table directly.
    // Otherwise, use the owners table path which includes fungibles.
    let (assets, grand_total) = if options.show_fungible || token_type.is_some() {
        let (sort_direction, sort_column) = create_owner_sorting(sort_by);
        scopes::asset::get_by_owner(
            db,
            owner_address,
            sort_column,
            sort_direction,
            &pagination,
            page_options.limit,
            enable_grand_total_query,
            options,
            token_type,
            collection_id,
            asset_id,
        )
        .await?
    } else {
        let (sort_direction, sort_column) = create_sorting(sort_by);
        scopes::asset::get_assets_by_owner(
            db,
            owner_address,
            sort_column,
            sort_direction,
            &pagination,
            page_options.limit,
            enable_grand_total_query,
            options,
        )
        .await?
    };

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
