use sea_orm_migration::prelude::*;

use crate::model::table::{Asset, AssetDataV2};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AssetDataV2::Table)
                    .add_column(ColumnDef::new(AssetDataV2::BaseInfoSeq).big_integer())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Asset::Table)
                    .add_column(ColumnDef::new(Asset::BaseInfoSeq).big_integer())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AssetDataV2::Table)
                    .drop_column(AssetDataV2::BaseInfoSeq)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Asset::Table)
                    .drop_column(Asset::BaseInfoSeq)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
