use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ClAudits::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ClAudits::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment(),
                    )
                    .col(ColumnDef::new(ClAudits::Tree).binary().not_null())
                    .col(ColumnDef::new(ClAudits::NodeIdx).big_integer().not_null())
                    .col(ColumnDef::new(ClAudits::LeafIdx).big_integer())
                    .col(ColumnDef::new(ClAudits::Seq).big_integer().not_null())
                    .col(ColumnDef::new(ClAudits::Level).big_integer().not_null())
                    .col(ColumnDef::new(ClAudits::Hash).binary().not_null())
                    .col(
                        ColumnDef::new(ClAudits::CreatedAt)
                            .date_time()
                            .default(SimpleExpr::Keyword(Keyword::CurrentTimestamp))
                            .not_null(),
                    )
                    .col(ColumnDef::new(ClAudits::Tx).string().not_null())
                    .to_owned(),
            )
            .await?;

        manager
        .get_connection()
        .execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "
            ALTER TABLE cl_audits ADD CONSTRAINT unique_tree_tx_nodeidx_seq UNIQUE (tree, node_idx, seq, tx);
            ".to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                "
        ALTER TABLE cl_audits DROP CONSTRAINT unique_tree_tx_nodeidx_seq;
        "
                .to_string(),
            ))
            .await?;
        manager
            .drop_table(Table::drop().table(ClAudits::Table).to_owned())
            .await?;
        Ok(())
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
enum ClAudits {
    Table,
    Id,
    Tree,
    NodeIdx,
    LeafIdx,
    Seq,
    Level,
    Hash,
    CreatedAt,
    Tx,
}
