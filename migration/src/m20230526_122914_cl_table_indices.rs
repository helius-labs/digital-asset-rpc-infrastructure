use digital_asset_types::dao::cl_audits;
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
            ALTER TABLE cl_audits ADD CONSTRAINT unique_tree_tx_nodeidx_seq UNIQUE (tree, node_idx, leaf_idx, seq, level, hash, tx);
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
        Ok(())
    }
}
