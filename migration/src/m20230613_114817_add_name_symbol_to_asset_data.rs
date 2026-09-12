use sea_orm_migration::prelude::*;

use crate::model::table::AssetData;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                sea_query::Table::alter()
                    .table(AssetData::Table)
                    .add_column(ColumnDef::new(Alias::new("raw_name")).binary())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                sea_query::Table::alter()
                    .table(AssetData::Table)
                    .add_column(ColumnDef::new(Alias::new("raw_symbol")).binary())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                sea_query::Table::alter()
                    .table(AssetData::Table)
                    .drop_column(Alias::new("raw_name"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                sea_query::Table::alter()
                    .table(AssetData::Table)
                    .drop_column(Alias::new("raw_symbol"))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
