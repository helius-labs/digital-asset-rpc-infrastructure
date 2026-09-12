use sea_orm::DbErr;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::ConnectionTrait;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

async fn execute_sql<'a>(manager: &SchemaManager<'_>, sql: &str) -> Result<(), DbErr> {
    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            sql.to_string(),
        ))
        .await?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql_owners_index = "
            CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS idx_owners_mint_token_account 
            ON owners (mint, token_account);
        ";
        execute_sql(manager, sql_owners_index).await?;

        let sql_asset_creators_index = "
            CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_asset_creators_creator_asset_id 
            ON asset_creators (creator, asset_id);
        ";
        execute_sql(manager, sql_asset_creators_index).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql_remove_owners_index = "
            DROP INDEX CONCURRENTLY IF EXISTS idx_owners_mint_token_account;
        ";
        execute_sql(manager, sql_remove_owners_index).await?;
        let sql_remove_assetcreators_index = "
            DROP INDEX CONCURRENTLY IF EXISTS idx_asset_creators_creator_asset_id;
        ";
        execute_sql(manager, sql_remove_assetcreators_index).await?;
        Ok(())
    }
}
