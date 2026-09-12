use digital_asset_types::dao::tasks;
use log::info;
use nft_ingester::error::IngesterError;
use sea_orm::{DatabaseConnection, DeleteResult, EntityTrait};

use crate::args::Args;

pub async fn delete(conn: DatabaseConnection, _args: Args) -> () {
    info!("Deleting all existing tasks");

    // Delete all existing tasks
    let deleted_tasks: Result<DeleteResult, IngesterError> = tasks::Entity::delete_many()
        .exec(&conn)
        .await
        .map_err(|e| e.into());

    match deleted_tasks {
        Ok(result) => {
            info!("Deleted a number of tasks {}", result.rows_affected);
        }
        Err(e) => {
            info!("Error deleting tasks: {}", e);
        }
    }
}
