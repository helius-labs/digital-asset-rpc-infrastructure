//! Run against a disposable local PostgreSQL database:
//! DATABASE_TEST_URL=postgres://localhost/das_test cargo test -p nft_ingester --lib enrichment_skips -- --ignored
use super::{
    upsert_assets_mint_account_columns, upsert_assets_token_account_columns,
    AssetMintAccountColumns, AssetTokenAccountColumns,
};
use sea_orm::{ConnectionTrait, Database, DatabaseTransaction, DbBackend, Statement, TransactionTrait};
use serde_json::json;

async fn version(conn: &impl ConnectionTrait) -> String {
    conn.query_one(Statement::from_string(
        DbBackend::Postgres,
        "SELECT ctid::text AS version FROM asset".to_owned(),
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "version")
    .unwrap()
}

fn mint(supply: u64, slot: u64) -> AssetMintAccountColumns {
    AssetMintAccountColumns {
        mint: vec![1; 32],
        supply,
        supply_mint: Some(vec![1; 32]),
        slot_updated_mint_account: slot,
    }
}

fn owner(slot: Option<i64>) -> AssetTokenAccountColumns {
    AssetTokenAccountColumns {
        mint: vec![1; 32],
        owner: Some(vec![2; 32]),
        frozen: false,
        delegate: None,
        token_extensions: None,
        slot_updated_token_account: slot,
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_TEST_URL pointing to disposable local PostgreSQL"]
async fn enrichment_skips_identical_values_but_preserves_ordering_and_repairs() {
    let url = std::env::var("DATABASE_TEST_URL").expect("set DATABASE_TEST_URL");
    let parsed = url::Url::parse(&url).unwrap();
    assert!(matches!(parsed.host_str(), Some("localhost" | "127.0.0.1")));
    let db = Database::connect(url).await.unwrap();
    let txn = db.begin().await.unwrap();
    // The temporary relation shadows public.asset on this transaction's connection.
    // No production schema or migrations are required.
    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        r#"
        CREATE TEMP TABLE asset (
            id bytea PRIMARY KEY, supply bigint, supply_mint bytea,
            slot_updated_mint_account bigint, owner bytea, frozen boolean,
            delegate bytea, token_extensions jsonb, slot_updated_token_account bigint,
            slot_updated bigint, slot_updated_metadata_account bigint,
            slot_updated_cnft_transaction bigint, slot_updated_agent_registry bigint
        ) ON COMMIT DROP
    "#
        .to_owned(),
    ))
    .await
    .unwrap();

    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        r#"
        CREATE FUNCTION pg_temp.update_slot_updated() RETURNS trigger AS $$
        BEGIN
            NEW.slot_updated = GREATEST(NEW.slot_updated_token_account,
                NEW.slot_updated_mint_account, NEW.slot_updated_metadata_account,
                NEW.slot_updated_cnft_transaction, NEW.slot_updated_agent_registry);
            RETURN NEW;
        END;
        $$ LANGUAGE plpgsql;
    "#
        .to_owned(),
    ))
    .await
    .unwrap();
    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        r#"
        CREATE TRIGGER update_slot_updated_trigger BEFORE UPDATE ON asset
        FOR EACH ROW EXECUTE FUNCTION pg_temp.update_slot_updated();
    "#
        .to_owned(),
    ))
    .await
    .unwrap();

    upsert_assets_mint_account_columns(mint(1, 10), &txn)
        .await
        .unwrap();
    // INSERT does not run the production UPDATE trigger. An otherwise identical
    // replay must still repair the derived watermark before it can become a no-op.
    upsert_assets_mint_account_columns(mint(1, 10), &txn)
        .await
        .unwrap();
    let row = txn
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT slot_updated FROM asset".to_owned(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "slot_updated").unwrap(), 10);
    let initial = version(&txn).await;
    // Collect-triggered metadata enrichment reuses precisely these stored values.
    for _ in 0..3 {
        upsert_assets_mint_account_columns(mint(1, 10), &txn)
            .await
            .unwrap();
        assert_eq!(version(&txn).await, initial);
    }
    // Same-slot changes must still apply, and older slots must not overwrite them.
    upsert_assets_mint_account_columns(mint(2, 10), &txn)
        .await
        .unwrap();
    let changed = version(&txn).await;
    assert_ne!(changed, initial);
    upsert_assets_mint_account_columns(mint(3, 9), &txn)
        .await
        .unwrap();
    assert_eq!(version(&txn).await, changed);
    // A newer slot with the same supply must advance the ordering watermark.
    upsert_assets_mint_account_columns(mint(2, 11), &txn)
        .await
        .unwrap();
    let advanced = version(&txn).await;
    assert_ne!(advanced, changed);
    upsert_assets_mint_account_columns(mint(4, 10), &txn)
        .await
        .unwrap();
    assert_eq!(version(&txn).await, advanced);
    let mut changed_mint = mint(2, 11);
    changed_mint.supply_mint = None;
    upsert_assets_mint_account_columns(changed_mint, &txn)
        .await
        .unwrap();
    assert_ne!(version(&txn).await, advanced);

    upsert_assets_token_account_columns(owner(None), &txn)
        .await
        .unwrap();
    let initial = version(&txn).await;
    upsert_assets_token_account_columns(owner(None), &txn)
        .await
        .unwrap();
    assert_eq!(version(&txn).await, initial); // NULL-safe equality
    upsert_assets_token_account_columns(owner(Some(10)), &txn)
        .await
        .unwrap();
    let initial = version(&txn).await;
    upsert_assets_token_account_columns(owner(Some(10)), &txn)
        .await
        .unwrap();
    assert_eq!(version(&txn).await, initial);

    // Every projected field, including nullable fields, must permit same-slot changes.
    for field in 0..4 {
        let mut next = owner(Some(10));
        match field {
            0 => next.owner = None,
            1 => next.frozen = true,
            2 => next.delegate = Some(vec![3; 32]),
            _ => next.token_extensions = Some(json!({"transferHook": {"program": "test"}})),
        }
        let before = version(&txn).await;
        upsert_assets_token_account_columns(next, &txn)
            .await
            .unwrap();
        assert_ne!(version(&txn).await, before);
        upsert_assets_token_account_columns(owner(Some(10)), &txn)
            .await
            .unwrap();
    }
    let before = version(&txn).await;
    upsert_assets_token_account_columns(owner(Some(11)), &txn)
        .await
        .unwrap();
    let advanced = version(&txn).await;
    assert_ne!(advanced, before);
    let mut stale = owner(Some(10));
    stale.owner = None;
    upsert_assets_token_account_columns(stale, &txn)
        .await
        .unwrap();
    assert_eq!(version(&txn).await, advanced);
    // Existing NULL ordering semantics stay intact: NULL cannot overwrite a known slot.
    upsert_assets_token_account_columns(owner(None), &txn)
        .await
        .unwrap();
    assert_eq!(version(&txn).await, advanced);
    // Reproduce a derived watermark needing repair while agent-registry data is
    // newer than both projections. Both helpers must preserve the trigger's repair.
    for use_mint in [true, false] {
        txn.execute(Statement::from_string(
            DbBackend::Postgres,
            "ALTER TABLE asset DISABLE TRIGGER update_slot_updated_trigger".to_owned(),
        ))
        .await
        .unwrap();
        txn.execute(Statement::from_string(
            DbBackend::Postgres,
            "UPDATE asset SET slot_updated = 0, slot_updated_agent_registry = 50".to_owned(),
        ))
        .await
        .unwrap();
        txn.execute(Statement::from_string(
            DbBackend::Postgres,
            "ALTER TABLE asset ENABLE TRIGGER update_slot_updated_trigger".to_owned(),
        ))
        .await
        .unwrap();
        let before_repair = version(&txn).await;
        upsert_assets_mint_account_columns(mint(99, 9), &txn)
            .await
            .unwrap();
        upsert_assets_token_account_columns(owner(Some(9)), &txn)
            .await
            .unwrap();
        assert_eq!(version(&txn).await, before_repair);
        if use_mint {
            let mut replay = mint(2, 11);
            replay.supply_mint = None;
            upsert_assets_mint_account_columns(replay, &txn)
                .await
                .unwrap();
        } else {
            upsert_assets_token_account_columns(owner(Some(11)), &txn)
                .await
                .unwrap();
        }
        let row = txn
            .query_one(Statement::from_string(
                DbBackend::Postgres,
                "SELECT slot_updated FROM asset".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "slot_updated").unwrap(), 50);
        let repaired = version(&txn).await;
        upsert_assets_token_account_columns(owner(Some(11)), &txn)
            .await
            .unwrap();
        assert_eq!(version(&txn).await, repaired);
    }
    txn.rollback().await.unwrap();
}

use super::{
    download_task_warranted, guard_asset_data_v2_noop, upsert_assets_metadata_account_columns,
    AssetMetadataAccountColumns,
};
use digital_asset_types::dao::sea_orm_active_enums::{OwnerType, SpecificationAssetClass};

fn metadata_cols(royalty: i32, slot: u64) -> AssetMetadataAccountColumns {
    AssetMetadataAccountColumns {
        mint: vec![1; 32],
        metadata_account_id: vec![9; 32],
        owner_type: OwnerType::Single,
        specification_asset_class: Some(SpecificationAssetClass::Nft),
        royalty_amount: royalty,
        asset_data: Some(vec![1; 32]),
        slot_updated_metadata_account: slot,
        mpl_core_plugins: None,
        mpl_core_unknown_plugins: None,
        mpl_core_collection_num_minted: None,
        mpl_core_collection_current_size: None,
        mpl_core_plugins_json_version: None,
        mpl_core_external_plugins: None,
        mpl_core_unknown_external_plugins: None,
        is_agent: false,
        asset_signer: None,
    }
}

/// The fee-sweep case: metadata account touches that change no stored value
/// must not produce a new row version, even though each touch carries a newer
/// slot. Content changes and watermark ordering must keep working.
#[tokio::test]
#[ignore = "requires DATABASE_TEST_URL pointing to disposable local PostgreSQL"]
async fn metadata_enrichment_skips_noop_account_touches() {
    let url = std::env::var("DATABASE_TEST_URL").expect("set DATABASE_TEST_URL");
    let parsed = url::Url::parse(&url).unwrap();
    assert!(matches!(parsed.host_str(), Some("localhost" | "127.0.0.1")));
    let db = Database::connect(url).await.unwrap();

    // Session-level enum types (CREATE TYPE cannot be TEMP); idempotent for
    // repeated runs against the same disposable database.
    for stmt in [
        "DO $$ BEGIN CREATE TYPE owner_type AS ENUM ('unknown','token','single'); EXCEPTION WHEN duplicate_object THEN NULL; END $$",
        "DO $$ BEGIN CREATE TYPE specification_versions AS ENUM ('unknown','v0','v1','v2'); EXCEPTION WHEN duplicate_object THEN NULL; END $$",
        "DO $$ BEGIN CREATE TYPE specification_asset_class AS ENUM ('unknown','NFT','FUNGIBLE_TOKEN','FUNGIBLE_ASSET','PROGRAMMABLE_NFT'); EXCEPTION WHEN duplicate_object THEN NULL; END $$",
        "DO $$ BEGIN CREATE TYPE royalty_target_type AS ENUM ('unknown','creators','fanout','single'); EXCEPTION WHEN duplicate_object THEN NULL; END $$",
    ] {
        db.execute(Statement::from_string(DbBackend::Postgres, stmt.to_owned()))
            .await
            .unwrap();
    }

    let txn = db.begin().await.unwrap();
    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        r#"
        CREATE TEMP TABLE asset (
            id bytea PRIMARY KEY, metadata_account_id bytea, owner_type owner_type,
            specification_version specification_versions,
            specification_asset_class specification_asset_class,
            tree_id bytea, nonce bigint, seq bigint, leaf bytea,
            data_hash text, creator_hash text, compressed boolean, compressible boolean,
            royalty_target_type royalty_target_type, royalty_target bytea, royalty_amount integer,
            asset_data bytea, burnt boolean,
            mpl_core_plugins jsonb, mpl_core_unknown_plugins jsonb,
            mpl_core_collection_num_minted integer, mpl_core_collection_current_size integer,
            mpl_core_plugins_json_version integer,
            mpl_core_external_plugins jsonb, mpl_core_unknown_external_plugins jsonb,
            is_agent boolean, asset_signer bytea,
            slot_updated bigint, slot_updated_metadata_account bigint,
            slot_updated_token_account bigint, slot_updated_mint_account bigint,
            slot_updated_cnft_transaction bigint, slot_updated_agent_registry bigint
        ) ON COMMIT DROP
    "#
        .to_owned(),
    ))
    .await
    .unwrap();
    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        r#"
        CREATE FUNCTION pg_temp.update_slot_updated_md() RETURNS trigger AS $$
        BEGIN
            NEW.slot_updated = GREATEST(NEW.slot_updated_token_account,
                NEW.slot_updated_mint_account, NEW.slot_updated_metadata_account,
                NEW.slot_updated_cnft_transaction, NEW.slot_updated_agent_registry);
            RETURN NEW;
        END;
        $$ LANGUAGE plpgsql;
    "#
        .to_owned(),
    ))
    .await
    .unwrap();
    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        "CREATE TRIGGER update_slot_updated_md_trigger BEFORE UPDATE ON asset
         FOR EACH ROW EXECUTE FUNCTION pg_temp.update_slot_updated_md();"
            .to_owned(),
    ))
    .await
    .unwrap();

    upsert_assets_metadata_account_columns(metadata_cols(500, 10), &txn)
        .await
        .unwrap();
    // INSERT does not run the UPDATE trigger; an identical replay repairs the
    // derived watermark once, after which replays must become no-ops.
    upsert_assets_metadata_account_columns(metadata_cols(500, 10), &txn)
        .await
        .unwrap();
    let initial = version(&txn).await;

    // The sweep signature: identical stored values at strictly newer slots.
    for slot in [11u64, 12, 13] {
        upsert_assets_metadata_account_columns(metadata_cols(500, slot), &txn)
            .await
            .unwrap();
        assert_eq!(
            version(&txn).await,
            initial,
            "no-op touch at slot {slot} rewrote the row"
        );
    }
    // The watermark freezes at the last meaningful change.
    let row = txn
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT slot_updated_metadata_account FROM asset".to_owned(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<i64>("", "slot_updated_metadata_account")
            .unwrap(),
        10
    );

    // A real change at a newer slot must write and advance the watermark.
    upsert_assets_metadata_account_columns(metadata_cols(750, 14), &txn)
        .await
        .unwrap();
    let changed = version(&txn).await;
    assert_ne!(changed, initial);
    // Same-slot changes must still apply (two real changes in one slot).
    upsert_assets_metadata_account_columns(metadata_cols(800, 14), &txn)
        .await
        .unwrap();
    let same_slot = version(&txn).await;
    assert_ne!(same_slot, changed);
    // Older slots must not overwrite newer data.
    upsert_assets_metadata_account_columns(metadata_cols(999, 13), &txn)
        .await
        .unwrap();
    assert_eq!(version(&txn).await, same_slot);

    txn.rollback().await.unwrap();
}

#[test]
fn download_task_gate_requires_new_uri_or_changed_metadata() {
    assert!(download_task_warranted(1, 0), "new URI must create a task");
    assert!(
        download_task_warranted(0, 1),
        "changed metadata must create a task"
    );
    assert!(download_task_warranted(1, 1));
    assert!(
        !download_task_warranted(0, 0),
        "a touch that changed nothing must not create a task"
    );
}

#[test]
fn asset_data_guard_compares_content_not_watermark() {
    let sql = guard_asset_data_v2_noop("INSERT ...".to_owned());
    assert!(sql.contains("IS DISTINCT FROM"));
    // Ordering is still enforced through the watermark...
    assert!(sql.contains("excluded.slot_updated >= asset_data_v2.slot_updated"));
    // ...but the watermark itself must not be part of the distinctness tuple,
    // else every touch (always at a newer slot) would defeat the guard.
    let tuple = sql.split("IS DISTINCT FROM").next().unwrap();
    let tuple = &tuple[tuple.find("AND (").unwrap()..];
    assert!(
        !tuple.contains("slot_updated"),
        "watermark leaked into distinctness tuple"
    );
}

/// Recovery must survive the no-op gate: rows whose document was never
/// fetched (processing) or whose permanent failure is past the retry horizon
/// must be re-armed by a touch (affected row => task); fetched documents,
/// fresh failures, and Invalid Uri rows must not be.
#[tokio::test]
#[ignore = "requires DATABASE_TEST_URL pointing to disposable local PostgreSQL"]
async fn offchain_repair_rearms_only_unfetched_rows() {
    use digital_asset_types::dao::offchain_metadata;
    use sea_orm::sea_query::OnConflict;
    use sea_orm::{ActiveValue::Set, EntityTrait, QueryTrait};

    let url = std::env::var("DATABASE_TEST_URL").expect("set DATABASE_TEST_URL");
    let parsed = url::Url::parse(&url).unwrap();
    assert!(matches!(parsed.host_str(), Some("localhost" | "127.0.0.1")));
    let db = Database::connect(url).await.unwrap();
    db.execute(Statement::from_string(
        DbBackend::Postgres,
        "DO $$ BEGIN CREATE TYPE mutability AS ENUM ('immutable','mutable','unknown'); EXCEPTION WHEN duplicate_object THEN NULL; END $$".to_owned(),
    ))
    .await
    .unwrap();
    let txn = db.begin().await.unwrap();
    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        r#"
        CREATE TEMP TABLE offchain_metadata (
            metadata_url text PRIMARY KEY, metadata jsonb, mutability mutability,
            reindex boolean, updated_at timestamptz
        ) ON COMMIT DROP
    "#
        .to_owned(),
    ))
    .await
    .unwrap();
    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        r#"
        INSERT INTO offchain_metadata VALUES
        ('u_fetched',        '{"name":"ok"}',                 'mutable', false, now()),
        ('u_processing',     '"processing"',                  'mutable', false, NULL),
        ('u_processing_new', '"processing"',                  'mutable', false, now()),
        ('u_processing_old', '"processing"',                  'mutable', false, now() - interval '25 hours'),
        ('u_fresh_pf',       '{"error":"permanent_failure"}', 'mutable', false, now()),
        ('u_stale_pf',       '{"error":"permanent_failure"}', 'mutable', false, now() - interval '25 hours'),
        ('u_invalid',        '"Invalid Uri"',                 'mutable', false, now())
    "#
        .to_owned(),
    ))
    .await
    .unwrap();

    let touch = |uri: &str| {
        let mut q = offchain_metadata::Entity::insert(offchain_metadata::ActiveModel {
            metadata_url: Set(uri.to_string()),
            metadata: Set(json!("processing")),
            mutability: Set(digital_asset_types::dao::sea_orm_active_enums::Mutability::Mutable),
            reindex: Set(true),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::columns([offchain_metadata::Column::MetadataUrl])
                .update_columns([offchain_metadata::Column::Reindex])
                .to_owned(),
        )
        .build(DbBackend::Postgres);
        q.sql = super::guard_offchain_insert_repair(q.sql);
        q
    };

    for (uri, rearmed) in [
        ("u_fetched", false),
        ("u_processing", true),      // never probed: recover
        ("u_processing_new", false), // probed just now: wait out the horizon
        ("u_processing_old", true),  // horizon passed: probe again
        ("u_fresh_pf", false),
        ("u_stale_pf", true),
        ("u_invalid", false),
        ("u_brand_new", true), // plain insert: new URI always warrants a task
    ] {
        let rows = txn.execute(touch(uri)).await.unwrap().rows_affected();
        assert_eq!(rows > 0, rearmed, "unexpected repair outcome for {uri}");
    }
    // Re-armed rows carry reindex=true so the runner unconditionally refetches.
    let row = txn
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT count(*) AS n FROM offchain_metadata WHERE reindex".to_owned(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "n").unwrap(), 4);

    txn.rollback().await.unwrap();
}

/// The three remaining per-touch writes must also become no-ops on identical
/// content: per-position creators rows and the authorities/collections CASE
/// upsert (whose payloads embed a per-touch slot that the guard must strip).
#[tokio::test]
#[ignore = "requires DATABASE_TEST_URL pointing to disposable local PostgreSQL"]
async fn creators_and_authorities_skip_noop_touches() {
    use super::{
        guard_asset_creators_noop, guard_authorities_collections_noop, settle_asset_creators_positions,
    };

    let url = std::env::var("DATABASE_TEST_URL").expect("set DATABASE_TEST_URL");
    let db = Database::connect(url).await.unwrap();
    let txn = db.begin().await.unwrap();

    // -- asset_creators --
    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        r#"CREATE TEMP TABLE asset_creators (
            asset_id bytea, position smallint, creator bytea, share integer,
            verified boolean, seq bigint, slot_updated bigint,
            PRIMARY KEY (asset_id, position)) ON COMMIT DROP"#
            .to_owned(),
    ))
    .await
    .unwrap();
    let creators_sql = |share: i32, slot: i64| {
        guard_asset_creators_noop(format!(
            r#"INSERT INTO asset_creators (asset_id, position, creator, share, verified, seq, slot_updated)
               VALUES ('\x01', 0, '\x02', {share}, true, 0, {slot})
               ON CONFLICT (asset_id, position) DO UPDATE SET
               creator = excluded.creator, share = excluded.share, verified = excluded.verified,
               seq = excluded.seq, slot_updated = excluded.slot_updated"#
        ))
    };
    let ver = |t: &str| format!("SELECT ctid::text AS version FROM {t}");
    let run = |sql: String| Statement::from_string(DbBackend::Postgres, sql);

    txn.execute(run(creators_sql(50, 10))).await.unwrap();
    let v0 = txn
        .query_one(run(ver("asset_creators")))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "version")
        .unwrap();
    // Identical creators at a newer slot: the sweep signature. No row version.
    for slot in [11i64, 12] {
        txn.execute(run(creators_sql(50, slot))).await.unwrap();
        let v = txn
            .query_one(run(ver("asset_creators")))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "version")
            .unwrap();
        assert_eq!(v, v0, "no-op creators touch at slot {slot} rewrote the row");
    }
    // A real share change still writes; an older slot stays blocked.
    txn.execute(run(creators_sql(60, 12))).await.unwrap();
    let v1 = txn
        .query_one(run(ver("asset_creators")))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "version")
        .unwrap();
    assert_ne!(v1, v0);
    txn.execute(run(creators_sql(70, 9))).await.unwrap();
    let v2 = txn
        .query_one(run(ver("asset_creators")))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "version")
        .unwrap();
    assert_eq!(v2, v1);

    // Reorder regression (test_creators_reordering): the read path keeps only
    // rows at the max slot_updated, so when some positions change and others
    // do not, the unchanged ones must be realigned to the new slot.
    let two_creators = |a: &str, b: &str, slot: i64| {
        guard_asset_creators_noop(format!(
            r#"INSERT INTO asset_creators (asset_id, position, creator, share, verified, seq, slot_updated)
               VALUES ('\x0c', 0, '{a}', 50, true, 0, {slot}), ('\x0c', 1, '{b}', 50, true, 0, {slot})
               ON CONFLICT (asset_id, position) DO UPDATE SET
               creator = excluded.creator, share = excluded.share, verified = excluded.verified,
               seq = excluded.seq, slot_updated = excluded.slot_updated"#
        ))
    };
    txn.execute(run(two_creators("\\x0d", "\\x0e", 10)))
        .await
        .unwrap();
    settle_asset_creators_positions(&txn, vec![0x0c], 10, 2)
        .await
        .unwrap();
    // Change position 0 only; position 1 stays identical and would be skipped.
    txn.execute(run(two_creators("\\x0e", "\\x0e", 20)))
        .await
        .unwrap();
    settle_asset_creators_positions(&txn, vec![0x0c], 20, 2)
        .await
        .unwrap();
    let uniform = txn
        .query_one(run(
            "SELECT count(DISTINCT slot_updated) AS n, max(slot_updated) AS s FROM asset_creators WHERE asset_id = '\\x0c'"
                .to_owned(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        uniform.try_get::<i64>("", "n").unwrap(),
        1,
        "positions must share one slot after a partial write"
    );
    assert_eq!(uniform.try_get::<i64>("", "s").unwrap(), 20);
    // And a pure no-op touch at a newer slot still writes nothing at all.
    let before = txn
        .query_one(run(
            "SELECT ctid::text AS version FROM asset_creators WHERE asset_id = '\\x0c' AND position = 1"
                .to_owned(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "version")
        .unwrap();
    txn.execute(run(two_creators("\\x0e", "\\x0e", 30)))
        .await
        .unwrap();
    settle_asset_creators_positions(&txn, vec![0x0c], 30, 2)
        .await
        .unwrap();
    let after_row = txn
        .query_one(run(
            "SELECT ctid::text AS version, slot_updated FROM asset_creators WHERE asset_id = '\\x0c' AND position = 1"
                .to_owned(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_row.try_get::<String>("", "version").unwrap(), before);
    assert_eq!(
        after_row.try_get::<i64>("", "slot_updated").unwrap(),
        20,
        "no-op touch must not advance the slot"
    );

    // Shrink regression: a list that drops its tail must lose the removed
    // position outright. Leaving it behind only reads as stale while some
    // other position sits at a higher slot, which a shrink whose surviving
    // creators are unchanged never produces.
    let one_creator = |a: &str, share: i32, slot: i64| {
        guard_asset_creators_noop(format!(
            r#"INSERT INTO asset_creators (asset_id, position, creator, share, verified, seq, slot_updated)
               VALUES ('\x0c', 0, '{a}', {share}, true, 0, {slot})
               ON CONFLICT (asset_id, position) DO UPDATE SET
               creator = excluded.creator, share = excluded.share, verified = excluded.verified,
               seq = excluded.seq, slot_updated = excluded.slot_updated"#
        ))
    };
    async fn tail_rows(txn: &DatabaseTransaction) -> i64 {
        txn.query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT count(*) AS n FROM asset_creators WHERE asset_id = '\\x0c' AND position = 1"
                .to_owned(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap()
    }
    // Shrink while position 0 also changes: the write happens, tail goes.
    txn.execute(run(one_creator("\\x0f", 40, 40))).await.unwrap();
    settle_asset_creators_positions(&txn, vec![0x0c], 40, 1)
        .await
        .unwrap();
    assert_eq!(
        tail_rows(&txn).await,
        0,
        "removed tail position must not survive a shrink"
    );

    // Shrink while every surviving creator is unchanged: the guard skips the
    // only incoming position, so nothing marks the tail stale. It must still go.
    txn.execute(run(two_creators("\\x0f", "\\x11", 50)))
        .await
        .unwrap();
    settle_asset_creators_positions(&txn, vec![0x0c], 50, 2)
        .await
        .unwrap();
    assert_eq!(tail_rows(&txn).await, 1, "setup: tail position restored");
    txn.execute(run(one_creator("\\x0f", 50, 60))).await.unwrap();
    settle_asset_creators_positions(&txn, vec![0x0c], 60, 1)
        .await
        .unwrap();
    assert_eq!(
        tail_rows(&txn).await,
        0,
        "shrink with unchanged survivors must still drop the removed creator"
    );

    // -- authorities/collections --
    txn.execute(Statement::from_string(
        DbBackend::Postgres,
        r#"CREATE TEMP TABLE asset (
            id bytea PRIMARY KEY, authorities_info jsonb, authority_address bytea,
            authority_slot_updated bigint, authority_scopes jsonb, collections_info jsonb
        ) ON COMMIT DROP"#
            .to_owned(),
    ))
    .await
    .unwrap();
    let auth_sql = |authority: &str, verified: bool, slot: i64| {
        guard_authorities_collections_noop(format!(
            r#"INSERT INTO asset (id, authorities_info, authority_address, authority_slot_updated, authority_scopes, collections_info)
               VALUES ('\x0a',
                 jsonb_build_object('authority', '{authority}', 'seq', 0, 'slot_updated', {slot}),
                 '\x0b', {slot}, NULL,
                 jsonb_build_object('collection_id', 'C1', 'verified', {verified}, 'slot_updated', {slot}))
               ON CONFLICT (id) DO UPDATE SET
               authorities_info = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN COALESCE(excluded.authorities_info, '{{}}'::jsonb) ELSE asset.authorities_info END,
               authority_address = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN excluded.authority_address ELSE asset.authority_address END,
               authority_slot_updated = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN excluded.authority_slot_updated ELSE asset.authority_slot_updated END,
               authority_scopes = CASE WHEN COALESCE((excluded.authorities_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.authorities_info->>'slot_updated')::bigint, -1) THEN excluded.authority_scopes ELSE asset.authority_scopes END,
               collections_info = CASE WHEN COALESCE((excluded.collections_info->>'slot_updated')::bigint, -1) >= COALESCE((asset.collections_info->>'slot_updated')::bigint, -1) THEN COALESCE(excluded.collections_info, '{{}}'::jsonb) ELSE asset.collections_info END
               WHERE asset.id = excluded.id"#
        ))
    };
    txn.execute(run(auth_sql("A1", false, 10))).await.unwrap();
    let v0 = txn
        .query_one(run(ver("asset")))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "version")
        .unwrap();
    // Identical content, newer embedded slots — must not produce a row version.
    for slot in [11i64, 12] {
        txn.execute(run(auth_sql("A1", false, slot))).await.unwrap();
        let v = txn
            .query_one(run(ver("asset")))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "version")
            .unwrap();
        assert_eq!(
            v, v0,
            "no-op authority touch at slot {slot} rewrote the row"
        );
    }
    // Real changes still write: authority change, then collection verification.
    txn.execute(run(auth_sql("A2", false, 13))).await.unwrap();
    let v1 = txn
        .query_one(run(ver("asset")))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "version")
        .unwrap();
    assert_ne!(v1, v0);
    txn.execute(run(auth_sql("A2", true, 14))).await.unwrap();
    let v2 = txn
        .query_one(run(ver("asset")))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "version")
        .unwrap();
    assert_ne!(v2, v1);
    // Older slot with different content: both ordering conditions fail, no write.
    txn.execute(run(auth_sql("A3", false, 5))).await.unwrap();
    let v3 = txn
        .query_one(run(ver("asset")))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "version")
        .unwrap();
    assert_eq!(v3, v2);

    txn.rollback().await.unwrap();
}
