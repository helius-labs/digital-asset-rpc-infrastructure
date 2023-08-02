use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CollectionMetadata::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CollectionMetadata::CollectionNftPubkey)
                            .string()
                            .not_null()
                            .unique_key()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CollectionMetadata::CollectionName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CollectionMetadata::Symbol)
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CollectionMetadata::Table).to_owned())
            .await?;
        Ok(())
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
enum CollectionMetadata {
    Table,
    CollectionNftPubkey,
    CollectionName,
    Symbol,
}
