use sea_orm::Statement;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DatabaseBackend};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP TABLE IF EXISTS asset_v1_account_attachments;".to_string(),
            ))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE TABLE IF NOT EXISTS asset_v1_account_attachments
                (
                    id              BYTEA PRIMARY KEY,
                    asset_id        BYTEA REFERENCES asset (id),
                    attachment_type V1_ACCOUNT_ATTACHMENTS NOT NULL,
                    initialized     BOOL NOT NULL DEFAULT false,
                    data            JSONB,
                    slot_updated    BIGINT NOT NULL
                )"
                .to_string(),
            ))
            .await?;
        Ok(())
    }
}
