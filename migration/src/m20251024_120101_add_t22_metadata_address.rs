use sea_orm_migration::{
    prelude::*,
    sea_orm::{ConnectionTrait, DatabaseBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add t22_metadata_address column to asset table
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset ADD COLUMN IF NOT EXISTS t22_metadata_address BYTEA;"
                    .to_string(),
            ))
            .await?;

        // Create index CONCURRENTLY to avoid blocking production
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS asset_t22_metadata_address_idx ON asset(t22_metadata_address) WHERE t22_metadata_address IS NOT NULL;"
                    .to_string(),
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop index
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX IF EXISTS asset_t22_metadata_address_idx;".to_string(),
            ))
            .await?;

        // Drop column
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset DROP COLUMN IF EXISTS t22_metadata_address;".to_string(),
            ))
            .await?;

        Ok(())
    }
}
