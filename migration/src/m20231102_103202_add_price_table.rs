use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Price::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Price::Mint).binary().primary_key())
                    .col(ColumnDef::new(Price::Symbol).string())
                    .col(ColumnDef::new(Price::Price).float())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Price::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Price {
    Table,
    Mint,
    Symbol,
    Price,
}
