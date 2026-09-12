use crate::args::Args;
use sea_orm::DatabaseConnection;

pub async fn show(_conn: DatabaseConnection, _args: Args) -> () {
    // TODO: Needs to be refactored after re-adding "force-reindex".

    // // Check the total number of assets in the DB
    // let condition_found =
    //     asset_data::Column::Metadata.ne(JsonValue::String("processing".to_string()));
    // let condition_missing =
    //     asset_data::Column::Metadata.eq(JsonValue::String("processing".to_string()));

    // let asset_data_finished = find_by_type(
    //     authority,
    //     collection,
    //     creator,
    //     mint,
    //     condition_found,
    //     include_url,
    //     ignore_url,
    // );
    // let asset_data_processing = find_by_type(
    //     authority,
    //     collection,
    //     creator,
    //     mint,
    //     condition_missing,
    //     include_url,
    //     ignore_url,
    // );
    // let asset_data_reindex = find_by_type(
    //     authority,
    //     collection,
    //     creator,
    //     mint,
    //     condition_reindex,
    //     include_url,
    //     ignore_url,
    // );

    // let mut asset_data_missing = asset_data_processing
    //     .0
    //     .order_by(asset_data::Column::Id, Order::Asc)
    //     .paginate(&conn, *batch_size)
    //     .into_stream();

    // let asset_data_count = asset_data_finished.0.count(&conn).await;
    // let asset_reindex_count = asset_data_reindex.0.count(&conn).await;

    // let mut i = 0;
    // while let Some(assets) = asset_data_missing.try_next().await.unwrap() {
    //     info!("Found {} assets", assets.len());
    //     i += assets.len();
    //     if let Some(matches) = matches.subcommand_matches("show") {
    //         if matches.get_flag("print") {
    //             for asset in assets {
    //                 info!(
    //                     "{}, missing asset, {:?}",
    //                     asset_data_processing.1,
    //                     Pubkey::try_from(asset.id)
    //                 );
    //             }
    //         }
    //     }
    // }

    // let total_finished = asset_data_count.unwrap_or(0);
    // let total_assets = i + total_finished as usize;
    // info!(
    //     "{}, reindexing assets: {:?}, total finished assets: {}, missing assets: {}, total assets: {}",
    //     asset_data_processing.1,
    //     asset_reindex_count,
    //     total_finished,
    //     i,
    //     total_assets
    // );
}
