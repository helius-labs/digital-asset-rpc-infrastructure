use sea_orm_migration::prelude::*;

use crate::model::table::Asset;

#[derive(DeriveMigrationName)]
pub struct Migration;

/*
This index is being created to make the following query efficient.
This query is used in `util-jobs` to find all affected assets with NULL owner.
```
SELECT * FROM "public"."asset"
WHERE owner IS NULL
AND owner_type = 'single'
AND supply = 1
AND compressed = false
AND burnt = false;
```
*/

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    // CREATE INDEX asset_optimized_index
                    // ON public.asset USING btree (owner, owner_type, supply, compressed, burnt);
                    .name("asset_optimized_index")
                    .col(Asset::Owner)
                    .col(Asset::OwnerType)
                    .col(Asset::Supply)
                    .col(Asset::Compressed)
                    .col(Asset::Burnt)
                    .table(Asset::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("asset_optimized_index")
                    .table(Asset::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
