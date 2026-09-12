use sea_orm_migration::prelude::*;

use crate::model::table::AssetGrouping;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                sea_query::Table::alter()
                    .table(Alias::new("asset_grouping"))
                    .modify_column(ColumnDef::new(AssetGrouping::Verified).null())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                sea_query::Table::alter()
                    .table(Alias::new("asset_grouping"))
                    .modify_column(ColumnDef::new(AssetGrouping::Verified).not_null())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
