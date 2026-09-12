use crate::{
    config::IngesterConfig,
    error::IngesterError,
    program_transformers::bubblegum::{
        save_changelog_event, upsert_asset_with_leaf_info,
        upsert_asset_with_owner_and_delegate_info, upsert_asset_with_seq,
        upsert_owner_for_compressed,
    },
};
use blockbuster::{instruction::InstructionBundle, programs::bubblegum::BubblegumInstruction};
use sea_orm::{ConnectionTrait, TransactionTrait};

use super::NormalizedLeafFields;

pub async fn delegate<'c, T>(
    _config: &'c IngesterConfig,
    parsing_result: &BubblegumInstruction,
    bundle: &InstructionBundle<'c>,
    txn_or_conn: &'c T,
    instruction: &str,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    if let (Some(le), Some(cl)) = (&parsing_result.leaf_update, &parsing_result.tree_update) {
        let seq =
            save_changelog_event(cl, bundle.slot, bundle.txn_id, txn_or_conn, instruction).await?;

        let leaf = NormalizedLeafFields::from(&le.schema);

        let id_bytes = leaf.id.to_bytes();
        let owner_bytes = leaf.owner.to_bytes().to_vec();
        let delegate = if leaf.owner == leaf.delegate || leaf.delegate.to_bytes() == [0; 32] {
            None
        } else {
            Some(leaf.delegate.to_bytes().to_vec())
        };
        let tree_id = cl.id.to_bytes();

        // Begin a transaction.  If the transaction goes out of scope (i.e. one of the executions has
        // an error and this function returns it using the `?` operator), then the transaction is
        // automatically rolled back.
        let multi_txn = txn_or_conn.begin().await?;

        // Partial update of asset table with just leaf.
        upsert_asset_with_leaf_info(
            &multi_txn,
            id_bytes.to_vec(),
            cl.index as i64,
            tree_id.to_vec(),
            le.leaf_hash.to_vec(),
            leaf.data_hash,
            leaf.creator_hash,
            leaf.collection_hash,
            leaf.asset_data_hash,
            leaf.flags,
            seq as i64,
        )
        .await?;

        // Partial update of asset table with just leaf owner and delegate.
        upsert_asset_with_owner_and_delegate_info(
            &multi_txn,
            id_bytes.to_vec(),
            owner_bytes.clone(),
            delegate.clone(),
            seq as i64,
        )
        .await?;

        upsert_asset_with_seq(&multi_txn, id_bytes.to_vec(), seq as i64).await?;

        upsert_owner_for_compressed(
            &multi_txn,
            id_bytes.to_vec(),
            owner_bytes,
            delegate,
            seq as i64,
        )
        .await?;

        multi_txn.commit().await?;

        return Ok(());
    }
    Err(IngesterError::ParsingError(
        "Ix not parsed correctly".to_string(),
    ))
}
