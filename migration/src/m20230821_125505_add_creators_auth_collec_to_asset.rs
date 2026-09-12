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
                "ALTER TABLE asset ADD COLUMN creators_info jsonb;".to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset ADD COLUMN collections_info jsonb;".to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset ADD COLUMN authorities_info jsonb;".to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX idx_asset_creators_info_gin ON asset USING gin(creators_info);"
                    .to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX idx_asset_collections_info_gin ON asset USING gin(collections_info);"
                    .to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX idx_asset_authorities_info_gin ON asset USING gin(authorities_info);"
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
                "DROP INDEX idx_asset_creators_info_gin;".to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX idx_asset_collections_info_gin;".to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX idx_asset_authorities_info_gin;".to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset DROP COLUMN creators_info;".to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset DROP COLUMN collections_info;".to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE asset DROP COLUMN authorities_info;".to_string(),
            ))
            .await?;

        Ok(())
    }
}
