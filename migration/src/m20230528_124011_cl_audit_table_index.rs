use sea_orm_migration::prelude::*;

use crate::model::table::ClAudits;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_cl_audits_tree")
                    .col(ClAudits::Tree)
                    .table(ClAudits::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_cl_audits_leaf_id")
                    .col(ClAudits::LeafIdx)
                    .table(ClAudits::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_cl_audits_tree")
                    .table(ClAudits::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_cl_audits_leaf_id")
                    .table(ClAudits::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
