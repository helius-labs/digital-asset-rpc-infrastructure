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
        // Backs `getAssetsByGroup` with groupKey "group", which matches
        // `collections_info -> 'groups' @> '[...]'`. Without it the query
        // seq-scans the asset table. CONCURRENTLY to avoid locking writes.
        execute_sql(
            manager,
            "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_asset_collections_info_groups_gin \
             ON asset USING gin ((collections_info -> 'groups'));",
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        execute_sql(
            manager,
            "DROP INDEX CONCURRENTLY IF EXISTS idx_asset_collections_info_groups_gin;",
        )
        .await
    }
}
