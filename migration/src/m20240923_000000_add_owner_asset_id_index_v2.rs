use sea_orm::DbErr;
use sea_orm_migration::prelude::*;

use crate::execute_sql;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql_assets_owner_asset_id_index = "
            CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS idx_asset_owner_asset_id
            ON asset (owner, id);
        ";
        execute_sql(manager, sql_assets_owner_asset_id_index).await?;

        let sql_assets_owner_compressed_id_index = "
            CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS idx_asset_owner_compressed_asset_id
            ON asset (owner, compressed, id);
        ";
        execute_sql(manager, sql_assets_owner_compressed_id_index).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql_remove_assets_owner_asset_id_index = "
            DROP INDEX CONCURRENTLY IF EXISTS idx_asset_owner_asset_id;
        ";
        execute_sql(manager, sql_remove_assets_owner_asset_id_index).await?;

        let sql_remove_assets_owner_compressed_asset_id_index = "
            DROP INDEX CONCURRENTLY IF EXISTS idx_asset_owner_compressed_asset_id;
        ";
        execute_sql(manager, sql_remove_assets_owner_compressed_asset_id_index).await?;

        Ok(())
    }
}
