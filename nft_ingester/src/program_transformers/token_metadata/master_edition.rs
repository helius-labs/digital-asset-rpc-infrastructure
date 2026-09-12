use crate::error::IngesterError;
use blockbuster::token_metadata::accounts::{DeprecatedMasterEditionV1, Edition, MasterEdition};
use blockbuster::token_metadata::types::Key;
use digital_asset_types::dao::editions;
use digital_asset_types::dao::sea_orm_active_enums::EditionAccountType;
use plerkle_serialization::Pubkey as FBPubkey;
use sea_orm::DbBackend;
use sea_orm::{query::*, sea_query::OnConflict, ActiveValue::Set, ConnectionTrait, EntityTrait};

pub async fn save_printable_edition<T: ConnectionTrait + TransactionTrait>(
    id: FBPubkey,
    edition: EditionAccountType,
    slot: u64,
    me_data: &Edition,
    conn: &T,
) -> Result<(), IngesterError> {
    let id_bytes = id.0.to_vec();

    let ser = serde_json::to_value(me_data)
        .map_err(|e| IngesterError::SerializatonError(e.to_string()))?;

    let model = editions::ActiveModel {
        id: Set(id_bytes),
        edition_type: Set(edition),
        parent: Set(Some(me_data.parent.to_bytes().to_vec())),
        data: Set(Some(ser)),
        slot_updated: Set(slot as i64),
    };

    let txn = conn.begin().await?;
    let mut query = editions::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([editions::Column::Id])
                .update_columns([
                    editions::Column::EditionType,
                    editions::Column::Parent,
                    editions::Column::Data,
                    editions::Column::SlotUpdated,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    query.sql = format!(
        "{} WHERE excluded.slot_updated >= editions.slot_updated",
        query.sql
    );

    txn.execute(query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    txn.commit().await?;
    Ok(())
}

pub async fn save_edition<T: ConnectionTrait + TransactionTrait>(
    id: FBPubkey,
    edition: EditionAccountType,
    slot: u64,
    me_data: &MasterEdition,
    conn: &T,
) -> Result<(), IngesterError> {
    let id_bytes = id.0.to_vec();

    let ser = serde_json::to_value(me_data)
        .map_err(|e| IngesterError::SerializatonError(e.to_string()))?;

    let model = editions::ActiveModel {
        id: Set(id_bytes),
        edition_type: Set(edition),
        data: Set(Some(ser)),
        slot_updated: Set(slot as i64),
        ..Default::default()
    };

    let txn = conn.begin().await?;
    let mut query = editions::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([editions::Column::Id])
                .update_columns([
                    editions::Column::EditionType,
                    editions::Column::Data,
                    editions::Column::SlotUpdated,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    query.sql = format!(
        "{} WHERE excluded.slot_updated >= editions.slot_updated",
        query.sql
    );

    txn.execute(query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    txn.commit().await?;
    Ok(())
}

pub async fn save_v1_edition<T: ConnectionTrait + TransactionTrait>(
    id: FBPubkey,
    slot: u64,
    me_data: &DeprecatedMasterEditionV1,
    conn: &T,
) -> Result<(), IngesterError> {
    // This discards the deprecated `MasterEditionV1` fields
    // but sets the `Key`` as `MasterEditionV1`.
    let bridge = MasterEdition {
        supply: me_data.supply,
        max_supply: me_data.max_supply,
        key: Key::MasterEditionV1,
    };
    save_edition(id, EditionAccountType::MasterEditionV1, slot, &bridge, conn).await
}
