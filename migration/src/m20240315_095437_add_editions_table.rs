use crate::model::table::{EditionAccountType, Editions};
use enum_iterator::all;
use sea_orm_migration::{
    prelude::*,
    sea_orm::{ConnectionTrait, DatabaseBackend, Statement},
    sea_query::extension::postgres::Type,
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();

        manager
            .create_type(
                Type::create()
                    .as_enum(Editions::EditionAccountType)
                    .values(vec![
                        EditionAccountType::Edition,
                        EditionAccountType::EditionMarker,
                        EditionAccountType::MasterEditionV1,
                        EditionAccountType::MasterEditionV2,
                        EditionAccountType::Unknown,
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Editions::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Editions::Id).binary().primary_key())
                    .col(ColumnDef::new(Editions::Parent).binary())
                    .col(ColumnDef::new(Editions::Data).json_binary())
                    .col(
                        ColumnDef::new(Editions::EditionType)
                            .enumeration(
                                Editions::EditionAccountType,
                                all::<EditionAccountType>().collect::<Vec<_>>(),
                            )
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Editions::SlotUpdated)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "CREATE INDEX idx_edition_parent ON editions(parent);".to_string(),
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "DROP INDEX idx_edition_parent;".to_string(),
            ))
            .await?;

        manager
            .drop_table(Table::drop().table(Editions::Table).to_owned())
            .await?;

        manager
            .drop_type(Type::drop().name(Editions::EditionAccountType).to_owned())
            .await?;
        Ok(())
    }
}
