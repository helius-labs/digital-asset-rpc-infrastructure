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

        // Create index concurrently to avoid blocking reads/writes
        // This index fixes searchAssets queries that filter by jsonUri (metadata_url)
        // which were causing full table scans and replication lag
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_asset_data_v2_metadata_url ON asset_data_v2(metadata_url);".to_string(),
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX IF EXISTS idx_asset_data_v2_metadata_url;".to_string(),
            ))
            .await?;

        Ok(())
    }
}
