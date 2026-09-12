use sea_orm::DbErr;
use sea_orm_migration::prelude::*;

use crate::model::table::Blocks;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Blocks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Blocks::Slot).big_integer().not_null())
                    .col(ColumnDef::new(Blocks::ParentSlot).big_integer().not_null())
                    .col(ColumnDef::new(Blocks::ParentBlockhash).binary().not_null())
                    .col(ColumnDef::new(Blocks::Blockhash).binary().not_null())
                    .col(ColumnDef::new(Blocks::BlockHeight).big_integer().not_null())
                    .col(ColumnDef::new(Blocks::BlockTime).big_integer().not_null())
                    .primary_key(Index::create().name("pk_blocks").col(Blocks::Slot))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Blocks::Table).to_owned())
            .await
    }
}
