use sea_orm_migration::{
    prelude::*,
    sea_orm::{ConnectionTrait, DatabaseBackend, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "
                DO $$ 
                DECLARE
                    type_exists BOOLEAN := EXISTS (SELECT 1 FROM pg_type WHERE typname = 'uint64_t');
                BEGIN
                    IF NOT type_exists THEN
                        CREATE DOMAIN uint64_t AS numeric(20, 0) CHECK (VALUE >= 0);
                    END IF;
                END $$;
                "
                    .to_string(),
            ))
            .await?;

        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE owners ADD COLUMN token_amount_u64 uint64_t;".to_string(),
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "ALTER TABLE owners DROP COLUMN token_amount_u64;".to_string(),
            ))
            .await?;

        Ok(())
    }
}
