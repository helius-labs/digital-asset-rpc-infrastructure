use super::NormalizedLeafFields;
use crate::{
    error::IngesterError,
    program_transformers::bubblegum::{
        bgum_use_method_to_token_metadata_use_method, save_changelog_event, upsert_asset_base_info,
        upsert_asset_creators, upsert_asset_data, upsert_asset_with_compression_info,
        upsert_asset_with_leaf_info, upsert_asset_with_owner_and_delegate_info,
        upsert_asset_with_seq, upsert_authority_info_in_asset, upsert_collection_info_in_asset,
        upsert_creators_info_in_asset, upsert_owner_for_compressed,
    },
    tasks::{DownloadMetadata, IntoTaskData, TaskData},
};
use blockbuster::token_metadata::types::TokenStandard as ChainTokenStandard;
use blockbuster::{
    instruction::InstructionBundle,
    programs::bubblegum::{BubblegumInstruction, Payload},
    token_metadata::types::Uses,
};
use mpl_bubblegum::types::{TokenStandard as MetadataTokenStandard, Version};

use chrono::Utc;
use digital_asset_types::{
    dao::sea_orm_active_enums::{
        ChainMutability, OwnerType, RoyaltyTargetType, SpecificationAssetClass,
        SpecificationVersions,
    },
    json::ChainDataV1,
};
use log::warn;
use sea_orm::{query::*, ConnectionTrait};

fn convert_to_chain_token_standard(
    token_standard: Option<MetadataTokenStandard>,
) -> Option<ChainTokenStandard> {
    match token_standard {
        Some(MetadataTokenStandard::NonFungible) => Some(ChainTokenStandard::NonFungible),
        Some(MetadataTokenStandard::FungibleAsset) => Some(ChainTokenStandard::FungibleAsset),
        Some(MetadataTokenStandard::Fungible) => Some(ChainTokenStandard::Fungible),
        Some(MetadataTokenStandard::NonFungibleEdition) => {
            Some(ChainTokenStandard::NonFungibleEdition)
        }
        None => Some(ChainTokenStandard::NonFungible),
    }
}

pub async fn mint<'c, T>(
    parsing_result: &BubblegumInstruction,
    bundle: &InstructionBundle<'c>,
    txn_or_conn: &'c T,
    instruction: &str,
    // ) -> ProgramTransformerResult<Option<DownloadMetadataInfo>>
) -> Result<Option<TaskData>, IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    if let (
        Some(le),
        Some(cl),
        Some(Payload::Mint {
            args,
            authority,
            tree_id,
        }),
    ) = (
        &parsing_result.leaf_update,
        &parsing_result.tree_update,
        &parsing_result.payload,
    ) {
        let seq =
            save_changelog_event(cl, bundle.slot, bundle.txn_id, txn_or_conn, instruction).await?;
        let metadata = args;

        let leaf = NormalizedLeafFields::from(&le.schema);

        let id_bytes = leaf.id.to_bytes();
        let slot_i = bundle.slot as i64;
        let uri = metadata.uri.trim().replace('\0', "");

        // Check if this is a Helium mint (authority is the Helium program)
        use solana_sdk::pubkey::Pubkey;
        let tree_pubkey = Pubkey::from(*tree_id);
        let auth_pubkey = Pubkey::from(*authority);
        let is_helium = auth_pubkey.to_string() == "memMa1HG4odAFmUbGWfPwS1WWfK95k99F2YTkGvyxZr";

        if is_helium {
            warn!(
                "HELIUM memMa1HG4: Minting asset {} from tree {} with authority {} in slot {}",
                leaf.id.to_string(),
                tree_pubkey.to_string(),
                auth_pubkey.to_string(),
                slot_i
            );
        }

        // Scam NFT
        if uri == "https://ipfs.io/ipfs/bafkreihxlhsvwlkrhbeg2edmmtvtarqxva46bhzvgxnszz2bvc4n7xyvg4"
        {
            return Ok(None);
        }
        let name = metadata.name.clone().into_bytes();
        let symbol = metadata.symbol.clone().into_bytes();
        let mut chain_data = ChainDataV1 {
            name: metadata.name.clone(),
            symbol: metadata.symbol.clone(),
            edition_nonce: metadata.edition_nonce,
            primary_sale_happened: metadata.primary_sale_happened,
            token_standard: convert_to_chain_token_standard(metadata.token_standard.clone()),
            uses: metadata.uses.clone().map(|u| Uses {
                use_method: bgum_use_method_to_token_metadata_use_method(u.use_method),
                remaining: u.remaining,
                total: u.total,
            }),
        };
        chain_data.sanitize();

        let chain_data_json = serde_json::to_value(chain_data)
            .map_err(|e| IngesterError::DeserializationError(e.to_string()))?;
        let chain_mutability = match metadata.is_mutable {
            true => ChainMutability::Mutable,
            false => ChainMutability::Immutable,
        };

        // Begin a transaction.  If the transaction goes out of scope (i.e. one of the executions has
        // an error and this function returns it using the `?` operator), then the transaction is
        // automatically rolled back.
        let multi_txn = txn_or_conn.begin().await?;

        upsert_asset_data(
            &multi_txn,
            id_bytes.to_vec(),
            chain_data_json,
            chain_mutability,
            uri.clone(),
            slot_i,
            name.to_vec(),
            symbol.to_vec(),
            seq as i64,
        )
        .await?;

        // Upsert `asset` table base info.
        let delegate = if leaf.owner == leaf.delegate || leaf.delegate.to_bytes() == [0; 32] {
            None
        } else {
            Some(leaf.delegate.to_bytes().to_vec())
        };

        // BubblegumV2 now gets its own `SpecificationAssetClass` and `Interface`.
        let specification_asset_class = if matches!(le.version, Version::V2) {
            SpecificationAssetClass::MplBubblegumV2
        } else {
            SpecificationAssetClass::Nft
        };

        // Upsert `asset` table base info and `asset_creators` table.
        upsert_asset_base_info(
            &multi_txn,
            id_bytes.to_vec(),
            OwnerType::Single,
            SpecificationVersions::V1,
            specification_asset_class,
            RoyaltyTargetType::Creators,
            None,
            metadata.seller_fee_basis_points as i32,
            slot_i,
            seq as i64,
        )
        .await?;

        // Partial update of asset table with just compression info elements.
        upsert_asset_with_compression_info(&multi_txn, id_bytes.to_vec(), true, false, 1, None)
            .await?;

        // Partial update of asset table with just leaf.
        upsert_asset_with_leaf_info(
            &multi_txn,
            id_bytes.to_vec(),
            leaf.nonce as i64,
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
            leaf.owner.to_bytes().to_vec(),
            delegate.clone(),
            seq as i64,
        )
        .await?;

        upsert_asset_with_seq(&multi_txn, id_bytes.to_vec(), seq as i64).await?;

        // Insert into `asset_authority` table.
        //TODO - we need to remove the optional bubblegum signer logic
        upsert_authority_info_in_asset(
            &multi_txn,
            id_bytes.to_vec(),
            authority.to_vec(),
            seq as i64,
            slot_i,
        )
        .await?;

        // Upsert into `asset_grouping` table with base collection info.
        upsert_collection_info_in_asset(
            &multi_txn,
            id_bytes.to_vec(),
            metadata.collection.clone(),
            slot_i,
            seq as i64,
        )
        .await?;

        upsert_owner_for_compressed(
            &multi_txn,
            id_bytes.to_vec(),
            leaf.owner.to_bytes().to_vec(),
            delegate,
            seq as i64,
        )
        .await?;

        // Upsert creators to `asset_creators` table.
        upsert_asset_creators(
            &multi_txn,
            id_bytes.to_vec(),
            &metadata.creators,
            slot_i,
            seq as i64,
        )
        .await?;

        // Upsert creators_info JSONB column for optimized queries.
        upsert_creators_info_in_asset(
            &multi_txn,
            id_bytes.to_vec(),
            &metadata.creators,
            slot_i,
            seq as i64,
        )
        .await?;

        multi_txn.commit().await?;

        if uri.is_empty() {
            warn!(
                "URI is empty for mint {}. Skipping background task.",
                bs58::encode(leaf.id).into_string()
            );
            return Ok(None);
        }

        let mut task = DownloadMetadata {
            asset_data_id: id_bytes.to_vec(),
            uri,
            created_at: Some(Utc::now().naive_utc()),
        };
        task.sanitize();
        let t = task.into_task_data()?;
        return Ok(Some(t));
    }
    Err(IngesterError::ParsingError(
        "Ix not parsed correctly".to_string(),
    ))
}
