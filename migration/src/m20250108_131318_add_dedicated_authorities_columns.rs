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
                "ALTER TABLE asset ADD COLUMN IF NOT EXISTS authority_address bytea;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset ADD COLUMN IF NOT EXISTS authority_scopes text[];".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset ADD COLUMN IF NOT EXISTS authority_slot_updated bigint;"
                    .to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset ADD COLUMN IF NOT EXISTS authority_seq bigint;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_asset_authority_address_id ON asset (authority_address, id);"
                    .to_string(),
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX CONCURRENTLY IF EXISTS idx_asset_authority_address_id;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset DROP COLUMN IF EXISTS authority_seq;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset DROP COLUMN IF EXISTS authority_slot_updated;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset DROP COLUMN IF EXISTS authority_scopes;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset DROP COLUMN IF EXISTS authority_address;".to_string(),
            ))
            .await?;

        Ok(())
    }
}
