use digital_asset_types::dao::{
    asset,
    sea_orm_active_enums::{OwnerType, RoyaltyTargetType, SpecificationAssetClass},
};
use sea_orm::{
    sea_query::OnConflict, ConnectionTrait, DbBackend, DbErr, EntityTrait, QueryTrait, Set,
    TransactionTrait,
};
use serde_json::Value;

pub struct AssetTokenAccountColumns {
    pub mint: Vec<u8>,
    pub owner: Option<Vec<u8>>,
    pub frozen: bool,
    pub delegate: Option<Vec<u8>>,
    pub token_extensions: Option<Value>,
    pub slot_updated_token_account: Option<i64>,
}

pub async fn upsert_assets_token_account_columns<T: ConnectionTrait + TransactionTrait>(
    columns: AssetTokenAccountColumns,
    txn_or_conn: &T,
) -> Result<(), DbErr> {
    let active_model = asset::ActiveModel {
        id: Set(columns.mint),
        owner: Set(columns.owner),
        frozen: Set(columns.frozen),
        delegate: Set(columns.delegate),
        token_extensions: Set(columns.token_extensions),
        slot_updated_token_account: Set(columns.slot_updated_token_account),
        ..Default::default()
    };
    let mut query = asset::Entity::insert(active_model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns([
                    asset::Column::Owner,
                    asset::Column::Frozen,
                    asset::Column::Delegate,
                    asset::Column::TokenExtensions,
                    asset::Column::SlotUpdatedTokenAccount,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    // Metadata indexing reuses the stored token account. Avoid creating another
    // row version when all values, including its ordering watermark, are identical.
    // Keep >=: two real changes can occur in the same slot. Permit repairs of
    // slot_updated too: the production BEFORE UPDATE trigger derives it from all sources.
    query.sql = format!(
    "{} WHERE (excluded.slot_updated_token_account >= asset.slot_updated_token_account OR asset.slot_updated_token_account IS NULL)
        AND ((excluded.owner, excluded.frozen, excluded.delegate, excluded.token_extensions, excluded.slot_updated_token_account)
            IS DISTINCT FROM (asset.owner, asset.frozen, asset.delegate, asset.token_extensions, asset.slot_updated_token_account)
        OR asset.slot_updated IS DISTINCT FROM GREATEST(asset.slot_updated_token_account,
            asset.slot_updated_mint_account, asset.slot_updated_metadata_account,
            asset.slot_updated_cnft_transaction, asset.slot_updated_agent_registry))",
    query.sql);
    txn_or_conn.execute(query).await?;
    Ok(())
}

pub struct AssetMintAccountColumns {
    pub mint: Vec<u8>,
    pub supply: u64,
    pub supply_mint: Option<Vec<u8>>,
    pub slot_updated_mint_account: u64,
}

pub async fn upsert_assets_mint_account_columns<T: ConnectionTrait + TransactionTrait>(
    columns: AssetMintAccountColumns,
    txn_or_conn: &T,
) -> Result<(), DbErr> {
    let active_model = asset::ActiveModel {
        id: Set(columns.mint),
        supply: Set(columns.supply as i64),
        supply_mint: Set(columns.supply_mint),
        slot_updated_mint_account: Set(Some(columns.slot_updated_mint_account as i64)),
        ..Default::default()
    };
    let mut query = asset::Entity::insert(active_model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns([
                    asset::Column::Supply,
                    asset::Column::SupplyMint,
                    asset::Column::SlotUpdatedMintAccount,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    // A metadata-only account touch must not rewrite the unchanged mint projection.
    // Include the slot so a newer watermark still advances even if supply is unchanged.
    // Preserve repairs performed by the production slot_updated trigger.
    query.sql = format!(
    "{} WHERE (excluded.slot_updated_mint_account >= asset.slot_updated_mint_account OR asset.slot_updated_mint_account IS NULL)
        AND ((excluded.supply, excluded.supply_mint, excluded.slot_updated_mint_account)
            IS DISTINCT FROM (asset.supply, asset.supply_mint, asset.slot_updated_mint_account)
        OR asset.slot_updated IS DISTINCT FROM GREATEST(asset.slot_updated_token_account,
            asset.slot_updated_mint_account, asset.slot_updated_metadata_account,
            asset.slot_updated_cnft_transaction, asset.slot_updated_agent_registry))",
    query.sql);
    txn_or_conn.execute(query).await?;
    Ok(())
}

pub struct AssetMetadataAccountColumns {
    pub mint: Vec<u8>,
    pub metadata_account_id: Vec<u8>,
    pub owner_type: OwnerType,
    pub specification_asset_class: Option<SpecificationAssetClass>,
    pub royalty_amount: i32,
    pub asset_data: Option<Vec<u8>>,
    pub slot_updated_metadata_account: u64,
    pub mpl_core_plugins: Option<Value>,
    pub mpl_core_unknown_plugins: Option<Value>,
    pub mpl_core_collection_num_minted: Option<i32>,
    pub mpl_core_collection_current_size: Option<i32>,
    pub mpl_core_plugins_json_version: Option<i32>,
    pub mpl_core_external_plugins: Option<Value>,
    pub mpl_core_unknown_external_plugins: Option<Value>,
    pub is_agent: bool,
    pub asset_signer: Option<Vec<u8>>,
}

pub async fn upsert_assets_metadata_account_columns<T: ConnectionTrait + TransactionTrait>(
    columns: AssetMetadataAccountColumns,
    txn_or_conn: &T,
) -> Result<(), DbErr> {
    let active_model = asset::ActiveModel {
        id: Set(columns.mint),
        metadata_account_id: Set(Some(columns.metadata_account_id)),
        owner_type: Set(columns.owner_type),
        specification_version: Set(Some(
            digital_asset_types::dao::sea_orm_active_enums::SpecificationVersions::V1,
        )),
        specification_asset_class: Set(columns.specification_asset_class),
        tree_id: Set(None),
        nonce: Set(Some(0)),
        seq: Set(Some(0)),
        leaf: Set(None),
        data_hash: Set(None),
        creator_hash: Set(None),
        compressed: Set(false),
        compressible: Set(false),
        royalty_target_type: Set(RoyaltyTargetType::Creators),
        royalty_target: Set(None),
        royalty_amount: Set(columns.royalty_amount),
        asset_data: Set(columns.asset_data),
        slot_updated_metadata_account: Set(Some(columns.slot_updated_metadata_account as i64)),
        burnt: Set(false),
        mpl_core_plugins: Set(columns.mpl_core_plugins),
        mpl_core_unknown_plugins: Set(columns.mpl_core_unknown_plugins),
        mpl_core_collection_num_minted: Set(columns.mpl_core_collection_num_minted),
        mpl_core_collection_current_size: Set(columns.mpl_core_collection_current_size),
        mpl_core_plugins_json_version: Set(columns.mpl_core_plugins_json_version),
        mpl_core_external_plugins: Set(columns.mpl_core_external_plugins),
        mpl_core_unknown_external_plugins: Set(columns.mpl_core_unknown_external_plugins),
        is_agent: Set(columns.is_agent),
        asset_signer: Set(columns.asset_signer),
        ..Default::default()
    };
    let mut query = asset::Entity::insert(active_model)
        .on_conflict(
            OnConflict::columns([asset::Column::Id])
                .update_columns([
                    asset::Column::MetadataAccountId,
                    asset::Column::OwnerType,
                    asset::Column::SpecificationVersion,
                    asset::Column::SpecificationAssetClass,
                    asset::Column::TreeId,
                    asset::Column::Nonce,
                    asset::Column::Seq,
                    asset::Column::Leaf,
                    asset::Column::DataHash,
                    asset::Column::CreatorHash,
                    asset::Column::Compressed,
                    asset::Column::Compressible,
                    asset::Column::RoyaltyTargetType,
                    asset::Column::RoyaltyTarget,
                    asset::Column::RoyaltyAmount,
                    asset::Column::AssetData,
                    asset::Column::SlotUpdatedMetadataAccount,
                    asset::Column::Burnt,
                    asset::Column::MplCorePlugins,
                    asset::Column::MplCoreUnknownPlugins,
                    asset::Column::MplCoreCollectionNumMinted,
                    asset::Column::MplCoreCollectionCurrentSize,
                    asset::Column::MplCorePluginsJsonVersion,
                    asset::Column::MplCoreExternalPlugins,
                    asset::Column::MplCoreUnknownExternalPlugins,
                    asset::Column::IsAgent,
                    asset::Column::AssetSigner,
                ])
                .to_owned(),
        )
        .build(DbBackend::Postgres);

    // Skip the write entirely when nothing we store would change. Metaplex
    // fee-collection sweeps rewrite metadata accounts without changing any
    // parsed field (only lamports / the fee flag move), and each such touch
    // previously produced a full row version + WAL on a 15+ column row.
    //
    // The watermark (slot_updated_metadata_account) is deliberately excluded
    // from the distinctness tuple: a touch always carries a newer slot, so
    // including it would defeat the guard. Consequence: the watermark freezes
    // at the slot of the last *meaningful* change, which the >= ordering
    // check still respects for future updates. The GREATEST clause preserves
    // repairs of slot_updated, mirroring the token/mint guards.
    query.sql = format!(
        "{} WHERE (excluded.slot_updated_metadata_account >= asset.slot_updated_metadata_account OR asset.slot_updated_metadata_account IS NULL)
            AND ((excluded.metadata_account_id, excluded.owner_type, excluded.specification_version, excluded.specification_asset_class,
                excluded.tree_id, excluded.nonce, excluded.seq, excluded.leaf, excluded.data_hash, excluded.creator_hash,
                excluded.compressed, excluded.compressible, excluded.royalty_target_type, excluded.royalty_target, excluded.royalty_amount,
                excluded.asset_data, excluded.burnt, excluded.mpl_core_plugins, excluded.mpl_core_unknown_plugins,
                excluded.mpl_core_collection_num_minted, excluded.mpl_core_collection_current_size, excluded.mpl_core_plugins_json_version,
                excluded.mpl_core_external_plugins, excluded.mpl_core_unknown_external_plugins, excluded.is_agent, excluded.asset_signer)
                IS DISTINCT FROM
                (asset.metadata_account_id, asset.owner_type, asset.specification_version, asset.specification_asset_class,
                asset.tree_id, asset.nonce, asset.seq, asset.leaf, asset.data_hash, asset.creator_hash,
                asset.compressed, asset.compressible, asset.royalty_target_type, asset.royalty_target, asset.royalty_amount,
                asset.asset_data, asset.burnt, asset.mpl_core_plugins, asset.mpl_core_unknown_plugins,
                asset.mpl_core_collection_num_minted, asset.mpl_core_collection_current_size, asset.mpl_core_plugins_json_version,
                asset.mpl_core_external_plugins, asset.mpl_core_unknown_external_plugins, asset.is_agent, asset.asset_signer)
            OR asset.slot_updated IS DISTINCT FROM GREATEST(asset.slot_updated_token_account,
                asset.slot_updated_mint_account, asset.slot_updated_metadata_account,
                asset.slot_updated_cnft_transaction, asset.slot_updated_agent_registry))",
        query.sql);
    txn_or_conn.execute(query).await?;
    Ok(())
}

/// Appends a content-distinctness guard to an `asset_data_v2` upsert so the
/// row is only written when a stored value actually changes. The watermark
/// (`slot_updated`) is excluded from the tuple for the same reason as above:
/// account touches always carry newer slots. Callers use the resulting
/// rows-affected count to decide whether a metadata re-download is warranted.
pub fn guard_asset_data_v2_noop(sql: String) -> String {
    format!(
        "{} WHERE excluded.slot_updated >= asset_data_v2.slot_updated
            AND (excluded.chain_mutability, excluded.chain_data, excluded.metadata_url, excluded.base_info_seq, excluded.raw_name, excluded.raw_symbol)
                IS DISTINCT FROM
                (asset_data_v2.chain_mutability, asset_data_v2.chain_data, asset_data_v2.metadata_url, asset_data_v2.base_info_seq, asset_data_v2.raw_name, asset_data_v2.raw_symbol)",
        sql
    )
}

/// Makes an `offchain_metadata` insert double as a recovery probe. On
/// conflict the row is re-armed (`reindex = true`, counting as an affected
/// row and therefore warranting a task) only when the stored document was
/// never successfully fetched:
///
/// - `"processing"` — the initial fetch never completed (transient failures,
///   crashed runners). Before the no-op gate, any account touch re-created a
///   task and eventually recovered these rows; this preserves that path.
/// - a permanent-failure marker older than the retry horizon — probed once
///   per horizon, matching the runner-side `permanent_failure_is_fresh`.
///
/// Fetched documents and fresh permanent failures are left untouched (no row
/// version, no task). `"Invalid Uri"` rows are never re-armed: the URI string
/// itself is the row key, and an unparseable URI cannot become fetchable.
pub fn guard_offchain_insert_repair(sql: String) -> String {
    let horizon = crate::tasks::common::PERMANENT_FAILURE_RETRY_HOURS;
    format!(
        "{} WHERE (offchain_metadata.metadata = '\"processing\"'::jsonb
                AND (offchain_metadata.updated_at IS NULL
                     OR offchain_metadata.updated_at < now() - interval '{} hours'))
            OR (offchain_metadata.metadata->>'error' = 'permanent_failure'
                AND (offchain_metadata.updated_at IS NULL
                     OR offchain_metadata.updated_at < now() - interval '{} hours'))",
        sql, horizon, horizon
    )
}

/// Appends a content guard to the per-position `asset_creators` upsert so a
/// touch that reasserts identical creators produces no row version. The
/// watermark is excluded from the tuple for the usual reason: touches always
/// carry newer slots.
///
/// IMPORTANT: the read path (`filter_out_stale_creators`) keeps only the rows
/// sharing the maximum `slot_updated` for the asset, so positions this guard
/// skips must not be left behind when *other* positions do write. Callers
/// must follow the guarded upsert with [`settle_asset_creators_positions`] in
/// the same transaction.
pub fn guard_asset_creators_noop(sql: String) -> String {
    format!(
        "{} WHERE (excluded.slot_updated >= asset_creators.slot_updated OR asset_creators.slot_updated IS NULL)
            AND ((excluded.creator, excluded.share, excluded.verified, excluded.seq)
                IS DISTINCT FROM
                (asset_creators.creator, asset_creators.share, asset_creators.verified, asset_creators.seq))",
        sql
    )
}

/// Settles the rows the guarded upsert did not touch, restoring the two
/// invariants the creators read path (`filter_out_stale_creators`) depends on.
///
/// Positions past the end of the incoming list are dropped: the read path
/// recognises them as stale only while they sit below the asset's maximum
/// `slot_updated`, which a shrink whose surviving creators are unchanged
/// never establishes, leaving the removed creator visible forever.
///
/// Surviving positions are then aligned to `slot` whenever any position was
/// written there, exactly as the unguarded upsert used to do.
///
/// A no-op touch writes nothing: there is no position to drop, and with no
/// row at `slot` the EXISTS matches nothing.
pub async fn settle_asset_creators_positions<T: ConnectionTrait>(
    txn_or_conn: &T,
    asset_id: Vec<u8>,
    slot: i64,
    creator_count: i16,
) -> Result<(), DbErr> {
    // `slot_updated <= $2` keeps an out-of-order replay carrying a shorter
    // list from deleting positions written by a newer update.
    let prune = sea_orm::Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        DELETE FROM asset_creators
        WHERE asset_id = $1 AND position >= $3 AND slot_updated <= $2
        "#,
        vec![
            asset_id.clone().into(),
            slot.into(),
            creator_count.into(),
        ],
    );
    txn_or_conn.execute(prune).await?;

    let realign = sea_orm::Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        UPDATE asset_creators SET slot_updated = $2
        WHERE asset_id = $1 AND slot_updated < $2 AND position < $3
          AND EXISTS (
            SELECT 1 FROM asset_creators
            WHERE asset_id = $1 AND slot_updated = $2
          )
        "#,
        vec![asset_id.into(), slot.into(), creator_count.into()],
    );
    txn_or_conn.execute(realign).await?;
    Ok(())
}

/// Appends a content guard to the authorities/collections asset upsert. The
/// embedded and column-level watermarks (`slot_updated` inside the JSON
/// payloads, `authority_slot_updated`) change on every touch, so they are
/// stripped from the comparison; the row is only written when the authority
/// or collection *content* an in-order update carries actually differs.
pub fn guard_authorities_collections_noop(sql: String) -> String {
    format!(
        "{} AND ((COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1)
                AND ((excluded.authorities_info - 'slot_updated'), excluded.authority_address, excluded.authority_scopes)
                    IS DISTINCT FROM
                    ((asset.authorities_info - 'slot_updated'), asset.authority_address, asset.authority_scopes))
            OR (COALESCE((excluded.collections_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.collections_info->>'slot_updated')::bigint, -1)
                AND (excluded.collections_info - 'slot_updated') IS DISTINCT FROM (asset.collections_info - 'slot_updated')))",
        sql
    )
}

/// A metadata re-download is warranted only when this account update
/// introduced a URI we have never stored, re-armed an unfetched row (see
/// [`guard_offchain_insert_repair`]), or changed stored metadata
/// (`asset_data_rows > 0` behind the content guard). A touch that did none of
/// these — e.g. a fee-collection sweep over already-indexed assets — is no
/// evidence the off-chain document changed, so no task is created for it.
pub fn download_task_warranted(offchain_rows: u64, asset_data_rows: u64) -> bool {
    offchain_rows > 0 || asset_data_rows > 0
}

#[cfg(test)]
#[path = "asset_enrichment_tests.rs"]
mod tests;
