use sea_orm::{DatabaseConnection, DbErr, EntityTrait, Order, QueryOrder};

use crate::dao::blocks;

pub async fn load_last_indexed_slot(db: &DatabaseConnection) -> Result<u64, DbErr> {
    let last_indexed_block = blocks::Entity::find()
        .order_by(blocks::Column::Slot, Order::Desc)
        .one(db)
        .await?;
    match last_indexed_block {
        Some(block) => Ok(block.slot as u64),
        None => Err(DbErr::RecordNotFound(
            "No last indexed block found".to_string(),
        )),
    }
}
