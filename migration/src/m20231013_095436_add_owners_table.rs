use sea_orm_migration::{
    prelude::*,
    sea_orm::{ConnectionTrait, DatabaseBackend, Statement},
};

use crate::model::table::Owners;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        manager
            .create_table(
                Table::create()
                    .table(Owners::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Owners::Owner).binary())
                    .col(ColumnDef::new(Owners::Mint).binary())
                    .col(ColumnDef::new(Owners::TokenAccount).binary())
                    .col(ColumnDef::new(Owners::Delegate).binary())
                    .col(ColumnDef::new(Owners::SlotUpdated).integer())
                    .col(ColumnDef::new(Owners::OwnerDelegateSeq).big_integer())
                    .to_owned(),
            )
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE owners ADD COLUMN created_at TIMESTAMP WITH TIME ZONE DEFAULT (now() AT TIME ZONE 'utc');".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE owners ADD CONSTRAINT unique_owner_mint UNIQUE (owner, mint);"
                    .to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE owners ADD CONSTRAINT unique_token_account UNIQUE (token_account);"
                    .to_string(),
            ))
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_owners_owner")
                    .table(Owners::Table)
                    .col(Owners::Owner)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_owners_mint")
                    .table(Owners::Table)
                    .col(Owners::Mint)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Owners::Table).to_owned())
            .await?;

        Ok(())
    }
}
