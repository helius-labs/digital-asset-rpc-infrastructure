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
                "
            ALTER TABLE asset 
            ADD COLUMN metadata_account_id bytea NULL;
            "
                .to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "
            CREATE UNIQUE INDEX idx_asset_metadata_account_id 
            ON asset USING btree(metadata_account_id);
            "
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
                "
            DROP INDEX IF EXISTS idx_asset_metadata_account_id;
            "
                .to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "
            ALTER TABLE asset 
            DROP COLUMN IF EXISTS metadata_account_id;
            "
                .to_string(),
            ))
            .await?;

        Ok(())
    }
}
