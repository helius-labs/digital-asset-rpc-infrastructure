use sea_orm_migration::{
    prelude::*,
    sea_orm::{ConnectionTrait, DatabaseBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE owners DROP CONSTRAINT unique_owner_mint;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE UNIQUE INDEX idx_unique_owner_mint_null_token ON owners(owner, mint) WHERE token_account IS NULL;".to_string()
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE owners DROP CONSTRAINT unique_owner_mint_token_account;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE owners ADD CONSTRAINT unique_owner_mint UNIQUE (owner, mint);"
                    .to_string(),
            ))
            .await?;

        Ok(())
    }
}
