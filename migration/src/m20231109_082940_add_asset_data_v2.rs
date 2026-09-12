use sea_orm_migration::{prelude::*, sea_orm::Iterable};

use crate::model::r#enum::Mutability;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create `offchain_metadata` table
        manager
            .create_table(
                Table::create()
                    .table(OffchainMetadata::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(OffchainMetadata::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(OffchainMetadata::MetadataUrl)
                            .string()
                            .unique_key()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OffchainMetadata::Mutability)
                            .enumeration(OffchainMetadata::Mutability, Mutability::iter())
                            .default(Mutability::Mutable.to_string())
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OffchainMetadata::Metadata)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OffchainMetadata::CreatedAt)
                            .timestamp_with_time_zone()
                            .default(SimpleExpr::Keyword(Keyword::CurrentTimestamp))
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OffchainMetadata::UpdatedAt)
                            .timestamp_with_time_zone()
                            .default(SimpleExpr::Keyword(Keyword::Null))
                            .null(),
                    )
                    .col(
                        ColumnDef::new(OffchainMetadata::Reindex)
                            .boolean()
                            .default(true)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Create `asset_data_v2` table
        manager
            .create_table(
                Table::create()
                    .table(AssetDataV2::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AssetDataV2::Id)
                            .binary()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AssetDataV2::MetadataUrl).string().not_null())
                    .col(
                        ColumnDef::new(AssetDataV2::ChainMutability)
                            .enumeration(AssetDataV2::ChainMutability, Mutability::iter())
                            .default(Mutability::Mutable.to_string())
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AssetDataV2::ChainData)
                            .json_binary()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AssetDataV2::RawName).binary())
                    .col(ColumnDef::new(AssetDataV2::RawSymbol).binary())
                    .col(
                        ColumnDef::new(AssetDataV2::SlotUpdated)
                            .big_integer()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_asset_data_v2_metadata_url")
                            .from(AssetDataV2::Table, AssetDataV2::MetadataUrl)
                            .to(OffchainMetadata::Table, OffchainMetadata::MetadataUrl),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AssetDataV2::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(OffchainMetadata::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(Iden)]
enum OffchainMetadata {
    Table,
    Id,
    MetadataUrl, // Unique
    Mutability,
    Metadata,
    CreatedAt,
    UpdatedAt,
    Reindex,
}

#[derive(Iden)]
enum AssetDataV2 {
    Table,
    Id,
    MetadataUrl, // FK
    ChainMutability,
    ChainData,
    RawName,
    RawSymbol,
    SlotUpdated,
}
