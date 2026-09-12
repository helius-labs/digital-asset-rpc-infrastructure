use sea_orm_migration::{
    prelude::*,
    sea_orm::{ConnectionTrait, DatabaseBackend, Statement},
};

use crate::model::table::Owners;

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
                    ALTER TABLE owners
                    ADD COLUMN id BIGSERIAL PRIMARY KEY;
                "
                .to_string(),
            ))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Owners::Table)
                    .drop_column(Owners::Id)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
