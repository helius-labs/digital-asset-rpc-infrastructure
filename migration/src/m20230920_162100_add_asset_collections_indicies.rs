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
                "CREATE INDEX idx_collections_info_id ON public.asset ((collections_info ->> 'collection_id'::text), id);"
                    .to_string(),
            ))
            .await?;
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX idx_collections_info_created ON public.asset ((collections_info ->> 'collection_id'::text), created_at);"
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
                "DROP INDEX idx_collections_info_id;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX idx_collections_info_created;".to_string(),
            ))
            .await?;

        Ok(())
    }
}
