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
                "CREATE INDEX idx_authorities_info_authority_id ON public.asset ((authorities_info -> 'authority'::text), id);"
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
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX idx_authorities_info_authority_id;".to_string(),
            ))
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX idx_authorities_info_authority_created;".to_string(),
            ))
            .await?;

        Ok(())
    }
}
