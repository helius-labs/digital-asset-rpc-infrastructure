use crate::error::IngesterError;
use digital_asset_types::dao::{
    asset, asset_creators, asset_data_v2, cl_audits_v2, cl_items, offchain_metadata, owners,
    sea_orm_active_enums::{
        ChainMutability, Instruction, Mutability, OwnerType, RoyaltyTargetType,
        SpecificationAssetClass, SpecificationVersions,
    },
    CollectionsInfo, CreatorInfo, CreatorsInfo,
};
use log::{debug, error};
use mpl_bubblegum::{
    types::{Collection, Creator},
    Flags,
};
use sea_orm::{query::*, sea_query::OnConflict, ActiveValue::Set, DbBackend, EntityTrait};
use sea_orm::{ActiveValue, ColumnTrait};
use mpl_account_compression::events::ChangeLogEventV1;

pub async fn save_changelog_event<'c, T>(
    change_log_event: &ChangeLogEventV1,
    slot: u64,
    txn_id: &str,
    txn_or_conn: &T,
    instruction: &str,
) -> Result<u64, IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    insert_change_log(change_log_event, slot, txn_id, txn_or_conn, instruction).await?;
    Ok(change_log_event.seq)
}

fn node_idx_to_leaf_idx(index: i64, tree_height: u32) -> i64 {
    index - 2i64.pow(tree_height)
}

pub async fn insert_change_log<'c, T>(
    change_log_event: &ChangeLogEventV1,
    _slot: u64,
    txn_id: &str,
    txn_or_conn: &T,
    instruction: &str,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let depth = change_log_event.path.len() - 1;
    let tree_id = change_log_event.id.as_ref();
    let mut items = Vec::new();

    for (i, p) in change_log_event.path.iter().enumerate() {
        let node_idx = p.index as i64;
        debug!(
            "seq {}, index {} level {}, node {}, txn {}, instruction {}",
            change_log_event.seq,
            p.index,
            i,
            bs58::encode(p.node).into_string(),
            txn_id,
            instruction
        );
        let leaf_idx = if i == 0 {
            Some(node_idx_to_leaf_idx(node_idx, depth as u32))
        } else {
            None
        };

        let item = cl_items::ActiveModel {
            tree: Set(tree_id.to_vec()),
            level: Set(i as i64),
            node_idx: Set(node_idx),
            hash: Set(p.node.as_ref().to_vec()),
            seq: Set(change_log_event.seq as i64),
            leaf_idx: Set(leaf_idx),
            ..Default::default()
        };

        items.push(item);
    }

    let mut query = cl_items::Entity::insert_many(items)
        .on_conflict(
            OnConflict::columns([cl_items::Column::Tree, cl_items::Column::NodeIdx])
                .update_columns([
                    cl_items::Column::Hash,
                    cl_items::Column::Seq,
                    cl_items::Column::LeafIdx,
                    cl_items::Column::Level,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    query.sql = format!("{} WHERE excluded.seq > cl_items.seq", query.sql);
    txn_or_conn
        .execute(query)
        .await
        .map_err(|db_err| IngesterError::StorageWriteError(db_err.to_string()))?;

    // New audit table that uses less storage.
    // Insert the audit item after the insert into cl_items have been completed
    let tx_id_bytes = bs58::decode(txn_id)
        .into_vec()
        .map_err(|_e| IngesterError::ChangeLogEventMalformed)?;
    let audit_item_v2 = cl_audits_v2::ActiveModel {
        tree: Set(tree_id.to_vec()),
        leaf_idx: Set(change_log_event.index as i64),
        seq: Set(change_log_event.seq as i64),
        tx: Set(tx_id_bytes),
        instruction: Set(Instruction::from(instruction)),
        ..Default::default()
    };
    let query = cl_audits_v2::Entity::insert(audit_item_v2)
        .on_conflict(
            OnConflict::columns([
                cl_audits_v2::Column::Tree,
                cl_audits_v2::Column::LeafIdx,
                cl_audits_v2::Column::Seq,
            ])
            .do_nothing()
            .to_owned(),
        )
        .build(DbBackend::Postgres);
    match txn_or_conn.execute(query).await {
        Ok(_) => {}
        Err(e) => {
            error!("Error while inserting into cl_audits_v2: {:?}", e);
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_asset_with_leaf_info<T>(
    txn_or_conn: &T,
    id: Vec<u8>,
    nonce: i64,
    tree_id: Vec<u8>,
    leaf: Vec<u8>,
    data_hash: [u8; 32],
    creator_hash: [u8; 32],
    collection_hash: Option<[u8; 32]>,
    asset_data_hash: Option<[u8; 32]>,
    flags: Option<u8>,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let data_hash = bs58::encode(data_hash).into_string().trim().to_string();
    let creator_hash = bs58::encode(creator_hash).into_string().trim().to_string();

    let mut model = asset::ActiveModel {
        id: Set(id),
        nonce: Set(Some(nonce)),
        tree_id: Set(Some(tree_id)),
        leaf: Set(Some(leaf)),
        data_hash: Set(Some(data_hash)),
        creator_hash: Set(Some(creator_hash)),
        leaf_seq: Set(Some(seq)),
        ..Default::default()
    };

    let mut update_columns = vec![
        asset::Column::Nonce,
        asset::Column::TreeId,
        asset::Column::Leaf,
        asset::Column::DataHash,
        asset::Column::CreatorHash,
        asset::Column::LeafSeq,
        asset::Column::Frozen,
    ];

    // Add V2 updates
    if let Some(flags) = flags {
        // Collection hash
        let collection_hash =
            collection_hash.map(|a| bs58::encode(a).into_string().trim().to_string());
        model.collection_hash = ActiveValue::Set(collection_hash);
        update_columns.push(asset::Column::CollectionHash);

        // Asset data hash
        let asset_data_hash =
            asset_data_hash.map(|a| bs58::encode(a).into_string().trim().to_string());
        model.asset_data_hash = ActiveValue::Set(asset_data_hash);
        update_columns.push(asset::Column::AssetDataHash);

        // Flags
        model.bubblegum_flags = ActiveValue::Set(Some(flags.into()));
        update_columns.push(asset::Column::BubblegumFlags);

        // Non-transferable
        let flags_bitfield = Flags::from_bytes([flags]);
        model.non_transferable = ActiveValue::Set(Some(flags_bitfield.non_transferable()));
        update_columns.push(asset::Column::NonTransferable);

        // Frozen
        let frozen = flags_bitfield.asset_lvl_frozen() || flags_bitfield.permanent_lvl_frozen();
        model.frozen = ActiveValue::Set(frozen);
    } else {
        // Default frozen to false.
        model.frozen = ActiveValue::Set(false)
    }

    let mut query = asset::Entity::insert(model)
        .on_conflict(
            OnConflict::column(asset::Column::Id)
                .update_columns(update_columns)
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    // Do not overwrite changes that happened after decompression (asset.seq = 0).
    // Do not overwrite changes from a later Bubblegum instruction.
    query.sql = format!(
        "{} WHERE (asset.seq != 0 OR asset.seq IS NULL) AND (excluded.leaf_seq >= asset.leaf_seq OR asset.leaf_seq IS NULL)",
        query.sql
    );

    txn_or_conn
        .execute(query)
        .await
        .map_err(|db_err| IngesterError::StorageWriteError(db_err.to_string()))?;

    Ok(())
}

pub async fn upsert_asset_with_owner_and_delegate_info<T>(
    txn_or_conn: &T,
    id: Vec<u8>,
    owner: Vec<u8>,
    delegate: Option<Vec<u8>>,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let model = asset::ActiveModel {
        id: Set(id),
        owner: Set(Some(owner)),
        delegate: Set(delegate),
        owner_delegate_seq: Set(Some(seq)),
        ..Default::default()
    };

    let mut query = asset::Entity::insert(model)
        .on_conflict(
            OnConflict::column(asset::Column::Id)
                .update_columns([
                    asset::Column::Owner,
                    asset::Column::Delegate,
                    asset::Column::OwnerDelegateSeq,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    // Do not overwrite changes that happened after decompression (asset.seq = 0).
    // Do not overwrite changes from a later Bubblegum instruction.
    query.sql = format!(
            "{} WHERE (asset.seq != 0 OR asset.seq IS NULL) AND (excluded.owner_delegate_seq >= asset.owner_delegate_seq OR asset.owner_delegate_seq IS NULL)",
            query.sql
        );

    txn_or_conn
        .execute(query)
        .await
        .map_err(|db_err| IngesterError::StorageWriteError(db_err.to_string()))?;

    Ok(())
}

pub async fn upsert_owner_for_core<T>(
    txn_or_conn: &T,
    mint: Vec<u8>,
    owner: Vec<u8>,
    delegate: Option<Vec<u8>>,
    slot: i64,
    frozen: bool,
    token_program: Vec<u8>,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    // HACK: This allows for a unlikely race condition. However, implementing performant locking would be a
    //       lot of work.
    let result = owners::Entity::find()
        .filter(
            owners::Column::Mint
                .eq(mint.clone())
                .and(owners::Column::TokenAccount.is_null())
                .and(owners::Column::SlotUpdated.gt(slot)),
        )
        .one(txn_or_conn)
        .await?;

    if result.is_some() {
        return Ok(());
    }

    // Usually owner rows are deleted through token account burns, but the Metaplex core NFTs
    // do not have associated token accounts.
    owners::Entity::delete_many()
        .filter(
            owners::Column::Mint
                .eq(mint.clone())
                .and(owners::Column::SlotUpdated.lt(slot))
                .and(owners::Column::TokenAccount.is_null()),
        )
        .exec(txn_or_conn)
        .await?;

    let delegate_value = match delegate {
        Some(val) => val.into(),
        None => Value::from(None::<Vec<u8>>),
    };

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        INSERT INTO "owners" ("mint", "owner", "delegate", "frozen", "token_amount_u64", "token_program", "slot_updated")
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT ("owner", "mint") WHERE token_account IS NULL
        DO UPDATE SET
        delegate = CASE
            WHEN owners.slot_updated IS NULL AND (excluded.slot_updated >= owners.slot_updated)
            THEN EXCLUDED.delegate
            ELSE owners.delegate
        END,
        slot_updated = CASE
            WHEN owners.slot_updated IS NULL AND (excluded.slot_updated >= owners.slot_updated)
            THEN EXCLUDED.slot_updated
            ELSE owners.slot_updated
        END,
        frozen = CASE
            WHEN owners.slot_updated IS NULL AND (excluded.slot_updated >= owners.slot_updated)
            THEN EXCLUDED.frozen
            ELSE owners.frozen
        END,
        token_program = CASE
            WHEN owners.slot_updated IS NULL AND (excluded.slot_updated >= owners.slot_updated)
            THEN EXCLUDED.token_program
            ELSE owners.token_program
        END
        "#,
        vec![
            mint.into(),
            owner.into(),
            delegate_value,
            frozen.into(),
            1.into(),
            token_program.into(),
            slot.into(),
        ],
    );

    txn_or_conn
        .execute(stmt)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    Ok(())
}

pub async fn upsert_owner_for_compressed<T>(
    txn_or_conn: &T,
    mint: Vec<u8>,
    owner: Vec<u8>,
    delegate: Option<Vec<u8>>,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let delegate_value = match delegate {
        Some(val) => val.into(),
        None => Value::from(None::<Vec<u8>>),
    };

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        INSERT INTO "owners" ("mint", "owner", "delegate", "owner_delegate_seq")
        VALUES ($1, $2, $3, $4)
        ON CONFLICT ("owner", "mint") WHERE token_account IS NULL
        DO UPDATE SET
        delegate = CASE
            WHEN owners.slot_updated IS NULL AND (excluded.owner_delegate_seq >= owners.owner_delegate_seq OR owners.owner_delegate_seq IS NULL)
            THEN EXCLUDED.delegate
            ELSE owners.delegate
        END,
        owner_delegate_seq = CASE
            WHEN owners.slot_updated IS NULL AND (excluded.owner_delegate_seq >= owners.owner_delegate_seq OR owners.owner_delegate_seq IS NULL)
            THEN EXCLUDED.owner_delegate_seq
            ELSE owners.owner_delegate_seq
        END
        "#,
        vec![mint.into(), owner.into(), delegate_value, seq.into()],
    );

    txn_or_conn
        .execute(stmt)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    Ok(())
}

pub async fn upsert_asset_with_compression_info<T>(
    txn_or_conn: &T,
    id: Vec<u8>,
    compressed: bool,
    compressible: bool,
    supply: i64,
    supply_mint: Option<Vec<u8>>,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let model = asset::ActiveModel {
        id: Set(id),
        compressed: Set(compressed),
        compressible: Set(compressible),
        supply: Set(supply),
        supply_mint: Set(supply_mint),
        ..Default::default()
    };

    let mut query = asset::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns([
                    asset::Column::Compressed,
                    asset::Column::Compressible,
                    asset::Column::Supply,
                    asset::Column::SupplyMint,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    // Do not overwrite changes that happened after decompression (asset.seq = 0).
    query.sql = format!("{} WHERE asset.seq != 0 OR asset.seq IS NULL", query.sql);
    txn_or_conn.execute(query).await?;

    Ok(())
}

// TODO: I believe we can be more efficient and include this along with the other updates.
// Also, what is seq even used for now that specific seqs?
// Maybe we can just include this in the cl_items updates.
pub async fn upsert_asset_with_seq<T>(
    txn_or_conn: &T,
    id: Vec<u8>,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let model = asset::ActiveModel {
        id: Set(id),
        seq: Set(Some(seq)),
        ..Default::default()
    };

    let mut query = asset::Entity::insert(model)
        .on_conflict(
            OnConflict::column(asset::Column::Id)
                .update_columns([asset::Column::Seq])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    // Do not overwrite changes that happened after decompression (asset.seq = 0).
    // Do not overwrite changes from a later Bubblegum instruction.
    query.sql = format!(
        "{} WHERE (asset.seq != 0 AND excluded.seq >= asset.seq) OR asset.seq IS NULL",
        query.sql
    );

    txn_or_conn
        .execute(query)
        .await
        .map_err(|db_err| IngesterError::StorageWriteError(db_err.to_string()))?;

    Ok(())
}

pub async fn upsert_collection_info_in_asset<T>(
    txn_or_conn: &T,
    asset_id: Vec<u8>,
    collection: Option<Collection>,
    slot_updated: i64,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let (collection_id, verified) = match collection {
        Some(c) => (Some(c.key.to_string()), c.verified),
        None => (None, false),
    };

    let collections_info = CollectionsInfo {
        collection_id,
        verified,
        slot_updated,
        collection_info_seq: Some(seq),
        seq: None, // Do we need to keep the seq value in CollectionsInfo? It seems to be unused in favour of collection_info_seq.
        collection_nft: None,
        collection_size: None,
        groups: Vec::new(),
    };

    let collections_info_json: serde_json::Value = collections_info.into();

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        INSERT INTO "asset" ("id", "collections_info")
        VALUES ($1, $2)
        ON CONFLICT ("id")
        DO UPDATE SET collections_info = CASE
            WHEN (asset.seq != 0 OR asset.seq IS NULL) AND
                ((excluded.collections_info->>'collection_info_seq')::bigint >= (asset.collections_info->>'collection_info_seq')::bigint OR (asset.collections_info->>'collection_info_seq') IS NULL)
            THEN excluded.collections_info
            ELSE asset.collections_info
        END
        WHERE asset.id = excluded.id
        "#,
        vec![asset_id.into(), Some(collections_info_json).into()],
    );

    txn_or_conn
        .execute(stmt)
        .await
        .map_err(|db_err| IngesterError::DatabaseError(db_err.to_string()))?;
    Ok(())
}

pub async fn upsert_authority_info_in_asset<T>(
    txn_or_conn: &T,
    asset_id: Vec<u8>,
    authority: Vec<u8>,
    seq: i64,
    slot_updated: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let authority_info_json = serde_json::json!({
        "authority": authority,
        "seq": seq,
        "slot_updated": slot_updated,
        "scopes": null,
    });

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
INSERT INTO "asset" ("id", "authorities_info", "authority_address", "authority_seq", "authority_slot_updated")
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (id)
DO UPDATE
SET
    authorities_info = EXCLUDED.authorities_info,
    authority_address = EXCLUDED.authority_address,
    authority_seq = EXCLUDED.authority_seq,
    authority_slot_updated = EXCLUDED.authority_slot_updated
WHERE asset.authorities_info IS NULL
OR NOT asset.authorities_info ? 'authority'
"#,
        vec![
            asset_id.into(),
            Some(authority_info_json).into(),
            authority.into(),
            seq.into(),
            slot_updated.into(),
        ],
    );

    // Execute the raw SQL
    txn_or_conn
        .execute(stmt)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_asset_base_info<T>(
    txn_or_conn: &T,
    id: Vec<u8>,
    owner_type: OwnerType,
    specification_version: SpecificationVersions,
    specification_asset_class: SpecificationAssetClass,
    royalty_target_type: RoyaltyTargetType,
    royalty_target: Option<Vec<u8>>,
    royalty_amount: i32,
    slot_updated: i64,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    // Set base info for asset.
    let asset_model = asset::ActiveModel {
        id: Set(id.clone()),
        owner_type: Set(owner_type),
        specification_version: Set(Some(specification_version)),
        specification_asset_class: Set(Some(specification_asset_class)),
        royalty_target_type: Set(royalty_target_type),
        royalty_target: Set(royalty_target),
        royalty_amount: Set(royalty_amount),
        asset_data: Set(Some(id.clone())),
        base_info_seq: Set(Some(seq)),
        slot_updated_cnft_transaction: Set(Some(slot_updated)),
        ..Default::default()
    };

    // Upsert asset table base info.
    let mut query = asset::Entity::insert(asset_model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns([
                    asset::Column::OwnerType,
                    asset::Column::SpecificationVersion,
                    asset::Column::SpecificationAssetClass,
                    asset::Column::RoyaltyTargetType,
                    asset::Column::RoyaltyTarget,
                    asset::Column::RoyaltyAmount,
                    asset::Column::AssetData,
                    asset::Column::SlotUpdatedCnftTransaction,
                    asset::Column::BaseInfoSeq,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    query.sql = format!(
            "{} WHERE (asset.seq != 0 OR asset.seq IS NULL) AND (excluded.base_info_seq >= asset.base_info_seq OR asset.base_info_seq IS NULL)",
            query.sql
        );

    txn_or_conn
        .execute(query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_asset_data<T>(
    txn_or_conn: &T,
    id: Vec<u8>,
    chain_data: JsonValue,
    chain_data_mutability: ChainMutability,
    metadata_url: String,
    slot_updated: i64,
    raw_name: Vec<u8>,
    raw_symbol: Vec<u8>,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    // The offchain JSON record needs to be inserted before asset_data_v2 due to a foreign key constraint.
    // Sequence counters / ordering does not matter for the offchain_metadata since its independent from the asset.
    let offchain_metadata_model = offchain_metadata::ActiveModel {
        metadata_url: Set(metadata_url.clone()),
        metadata: Set(JsonValue::String("processing".to_string())),
        mutability: Set(Mutability::Mutable),
        reindex: Set(true),
        ..Default::default()
    };

    let offchain_metadata_query = offchain_metadata::Entity::insert(offchain_metadata_model)
        .on_conflict(
            OnConflict::columns([offchain_metadata::Column::MetadataUrl])
                .do_nothing()
                .to_owned(),
        )
        .build(DbBackend::Postgres);
    txn_or_conn
        .execute(offchain_metadata_query)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;

    let model = asset_data_v2::ActiveModel {
        id: Set(id.clone()),
        chain_data: Set(chain_data),
        chain_mutability: Set(chain_data_mutability),
        metadata_url: Set(metadata_url),
        slot_updated: Set(slot_updated),
        raw_name: Set(Some(raw_name)),
        raw_symbol: Set(Some(raw_symbol)),
        base_info_seq: Set(Some(seq)),
    };
    let mut asset_data_query = asset_data_v2::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([asset_data_v2::Column::Id])
                .update_columns([
                    asset_data_v2::Column::ChainData,
                    asset_data_v2::Column::ChainMutability,
                    asset_data_v2::Column::MetadataUrl,
                    asset_data_v2::Column::SlotUpdated,
                    asset_data_v2::Column::RawName,
                    asset_data_v2::Column::RawSymbol,
                    asset_data_v2::Column::BaseInfoSeq,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    // Do not overwrite changes that happened after decompression (asset_data_v2.base_info_seq = 0).
    // Do not overwrite changes from a later Bubblegum instruction.
    asset_data_query.sql = format!(
        "{} WHERE (asset_data_v2.base_info_seq != 0 AND excluded.base_info_seq >= asset_data_v2.base_info_seq) OR asset_data_v2.base_info_seq IS NULL",
        asset_data_query.sql
    );
    txn_or_conn
        .execute(asset_data_query)
        .await
        .map_err(|db_err| IngesterError::StorageWriteError(db_err.to_string()))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_asset_creators<T>(
    txn_or_conn: &T,
    id: Vec<u8>,
    creators: &Vec<Creator>,
    slot_updated: i64,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let db_creators = if creators.is_empty() {
        // If creators are empty, insert an empty creator with the current sequence.
        // This prevents accidental errors during out-of-order updates.
        vec![asset_creators::ActiveModel {
            asset_id: Set(id.clone()),
            position: Set(0),
            creator: Set(vec![]),
            share: Set(100),
            verified: Set(false),
            slot_updated: Set(Some(slot_updated)),
            seq: Set(Some(seq)),
            ..Default::default()
        }]
    } else {
        creators
            .iter()
            .enumerate()
            .map(|(i, c)| asset_creators::ActiveModel {
                asset_id: Set(id.clone()),
                position: Set(i as i16),
                creator: Set(c.address.to_bytes().to_vec()),
                share: Set(c.share as i32),
                verified: Set(c.verified),
                slot_updated: Set(Some(slot_updated)),
                seq: Set(Some(seq)),
                ..Default::default()
            })
            .collect()
    };

    // This statement will update base information for each creator.
    let mut query = asset_creators::Entity::insert_many(db_creators)
        .on_conflict(
            OnConflict::columns([
                asset_creators::Column::AssetId,
                asset_creators::Column::Position,
            ])
            .update_columns([
                asset_creators::Column::Creator,
                asset_creators::Column::Share,
                asset_creators::Column::Verified,
                asset_creators::Column::Seq,
                asset_creators::Column::SlotUpdated,
            ])
            .to_owned(),
        )
        .build(DbBackend::Postgres);

    query.sql = format!(
        "{} WHERE (asset_creators.seq != 0 AND excluded.seq >= asset_creators.seq) OR asset_creators.seq IS NULL",
        query.sql
    );

    txn_or_conn.execute(query).await?;

    Ok(())
}

/// Upserts the creators_info JSONB column in the asset table.
/// This denormalizes creator data for faster queries by avoiding JOINs to asset_creators.
pub async fn upsert_creators_info_in_asset<T>(
    txn_or_conn: &T,
    asset_id: Vec<u8>,
    creators: &Vec<Creator>,
    slot_updated: i64,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let creator_infos: Vec<CreatorInfo> = creators
        .iter()
        .map(|c| CreatorInfo {
            creator: c.address.to_bytes().to_vec(),
            share: Some(c.share),
            verified: c.verified,
        })
        .collect();

    let creators_info = CreatorsInfo {
        creators: creator_infos,
        slot_updated: Some(slot_updated),
        seq: Some(seq),
    };

    let creators_info_json: serde_json::Value = creators_info.into();

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        INSERT INTO "asset" ("id", "creators_info")
        VALUES ($1, $2)
        ON CONFLICT ("id")
        DO UPDATE SET creators_info = CASE
            WHEN (asset.seq != 0 OR asset.seq IS NULL) AND
                ((excluded.creators_info->>'seq')::bigint >= (asset.creators_info->>'seq')::bigint OR (asset.creators_info->>'seq') IS NULL)
            THEN excluded.creators_info
            ELSE asset.creators_info
        END
        WHERE asset.id = excluded.id
        "#,
        vec![asset_id.into(), Some(creators_info_json).into()],
    );

    txn_or_conn
        .execute(stmt)
        .await
        .map_err(|db_err| IngesterError::DatabaseError(db_err.to_string()))?;
    Ok(())
}

/// Upserts the creators_info JSONB column using raw CreatorInfo data.
/// This variant is useful when creators are already in the internal format.
pub async fn upsert_creators_info_in_asset_raw<T>(
    txn_or_conn: &T,
    asset_id: Vec<u8>,
    creator_infos: Vec<CreatorInfo>,
    slot_updated: i64,
    seq: i64,
) -> Result<(), IngesterError>
where
    T: ConnectionTrait + TransactionTrait,
{
    let creators_info = CreatorsInfo {
        creators: creator_infos,
        slot_updated: Some(slot_updated),
        seq: Some(seq),
    };

    let creators_info_json: serde_json::Value = creators_info.into();

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        INSERT INTO "asset" ("id", "creators_info")
        VALUES ($1, $2)
        ON CONFLICT ("id")
        DO UPDATE SET creators_info = CASE
            WHEN (asset.seq != 0 OR asset.seq IS NULL) AND
                ((excluded.creators_info->>'seq')::bigint >= (asset.creators_info->>'seq')::bigint OR (asset.creators_info->>'seq') IS NULL)
            THEN excluded.creators_info
            ELSE asset.creators_info
        END
        WHERE asset.id = excluded.id
        "#,
        vec![asset_id.into(), Some(creators_info_json).into()],
    );

    txn_or_conn
        .execute(stmt)
        .await
        .map_err(|db_err| IngesterError::DatabaseError(db_err.to_string()))?;
    Ok(())
}
