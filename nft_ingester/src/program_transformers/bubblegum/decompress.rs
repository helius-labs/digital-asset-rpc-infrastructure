use crate::error::IngesterError;
use blockbuster::{instruction::InstructionBundle, programs::bubblegum::BubblegumInstruction};
use sea_orm::{query::*, ConnectionTrait};

use super::upsert_asset_with_leaf_info_for_decompression;

pub async fn decompress<'c, T>(
    _parsing_result: &BubblegumInstruction,
    bundle: &InstructionBundle<'c>,
    txn_or_conn: &'c T,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let id_bytes = bundle.keys.get(3).unwrap().0.as_slice();

    // Partial update of asset table with leaf and compression info.
    upsert_asset_with_leaf_info_for_decompression(txn_or_conn, id_bytes.to_vec()).await?;

    Ok(())
}
