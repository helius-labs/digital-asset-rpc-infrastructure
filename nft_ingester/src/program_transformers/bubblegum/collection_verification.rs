use crate::program_transformers::bubblegum::{upsert_asset_with_seq, upsert_collection_info};
use blockbuster::{
    instruction::InstructionBundle,
    programs::bubblegum::{BubblegumInstruction, LeafSchema, Payload},
};
use log::debug;
use mpl_bubblegum::{hash_metadata, state::metaplex_adapter::Collection};
use sea_orm::query::*;

use super::{save_changelog_event, upsert_asset_with_leaf_info};
use crate::error::IngesterError;
pub async fn process<'c, T>(
    parsing_result: &BubblegumInstruction,
    bundle: &InstructionBundle<'c>,
    txn: &'c T,
    instruction: &str,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    if let (Some(le), Some(cl), Some(payload)) = (
        &parsing_result.leaf_update,
        &parsing_result.tree_update,
        &parsing_result.payload,
    ) {
        let (collection, verify, metadata) = match payload {
            Payload::CollectionVerification {
                collection,
                verify,
                args,
            } => (collection.clone(), verify.clone(), args.clone()),
            _ => {
                return Err(IngesterError::ParsingError(
                    "Ix not parsed correctly".to_string(),
                ));
            }
        };
        debug!(
            "Handling collection verification event for {} (verify: {}): {}",
            collection, verify, bundle.txn_id
        );
        let seq = save_changelog_event(cl, bundle.slot, bundle.txn_id, txn, instruction).await?;
        let id_bytes = match le.schema {
            LeafSchema::V1 { id, .. } => id.to_bytes().to_vec(),
        };

        let mut updated_metadata = metadata.clone();
        updated_metadata.collection = Some(Collection {
            key: collection.clone(),
            verified: verify,
        });
        let updated_data_hash = hash_metadata(&updated_metadata)
            .map(|e| bs58::encode(e).into_string())
            .unwrap_or("".to_string())
            .trim()
            .to_string();

        // Partial update of asset table with just leaf.
        upsert_asset_with_leaf_info(
            txn,
            id_bytes.to_vec(),
            le.leaf_hash.to_vec(),
            Some(updated_data_hash),
            None,
            seq as i64,
            false,
        )
        .await?;

        upsert_asset_with_seq(txn, id_bytes.to_vec(), seq as i64).await?;

        // Nullify collection if it is not verified.
        // Bubblegum will soon support removing collections.
        let group_value = match verify {
            true => Some(collection.to_string()),
            false => None,
        };
        upsert_collection_info(
            txn,
            id_bytes.to_vec(),
            group_value,
            bundle.slot as i64,
            seq as i64,
        )
        .await?;

        return Ok(());
    };
    Err(IngesterError::ParsingError(
        "Ix not parsed correctly".to_string(),
    ))
}
