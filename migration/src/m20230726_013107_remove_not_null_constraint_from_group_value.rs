use digital_asset_types::dao::asset_grouping;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                sea_query::Table::alter()
                    .table(asset_grouping::Entity)
                    .modify_column(ColumnDef::new(asset_grouping::Column::GroupValue).null())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                sea_query::Table::alter()
                    .table(asset_grouping::Entity)
                    .modify_column(ColumnDef::new(asset_grouping::Column::GroupValue).not_null())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
