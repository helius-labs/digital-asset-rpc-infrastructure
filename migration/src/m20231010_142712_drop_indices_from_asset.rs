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
                "DROP INDEX idx_asset_creators_info_creators_gin;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX idx_authorities_info_authority_created;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX idx_asset_creators_info_gin;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX idx_collections_info_id;".to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX asset_leaf;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX asset_delegate;".to_string(),
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
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX idx_asset_creators_info_creators_gin ON asset USING GIN ((creators_info -> 'creators'));"
                    .to_string(),
            ))
            .await?;

        connection
        .execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE INDEX idx_authorities_info_authority_created ON public.asset ((authorities_info -> 'authority'::text), created_at);"
                .to_string(),
        ))
        .await?;

        connection
        .execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE INDEX idx_collections_info_id ON public.asset ((collections_info ->> 'collection_id'::text), id);"
                .to_string(),
        ))
        .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX asset_leaf ON public.asset USING btree (leaf);".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX asset_delegate aON public.asset USING btree (delegate);".to_string(),
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
}
