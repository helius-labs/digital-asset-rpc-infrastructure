use crate::error::IngesterError;
use blockbuster::token_metadata::{state::Metadata};
use digital_asset_types::dao::{asset_data};

use plerkle_serialization::Pubkey as FBPubkey;
use sea_orm::{entity::*, query::*, ActiveValue::Set, ConnectionTrait};

pub async fn save_v1_asset<T: ConnectionTrait + TransactionTrait>(
    _conn: &T,
    id: FBPubkey,
    _slot: u64,
    metadata: &Metadata,
) -> Result<(), IngesterError> {
    let metadata = metadata.clone();
    let data = metadata.data;
    let id = id.0;

    let name = data.name.clone().into_bytes();
    let symbol = data.symbol.clone().into_bytes();

    let _asset_data_model = asset_data::ActiveModel {
        id: Set(id.to_vec()),
        raw_name: Set(Some(name.to_vec())),
        raw_symbol: Set(Some(symbol.to_vec())),
        ..Default::default()
    };
    Ok(())

    /*
    let txn = conn.begin().await?;
    let query = asset::Entity::update(data)
        .filter(Condition::all().add(asset_data::Column::Id.eq(id.to_vec().clone())))
        .build(DbBackend::Postgres);
    txn.execute(query)
        .await
        .map(|_| ())
        .map_err(|e| IngesterError::DatabaseError(e.to_string()))
    */
}
