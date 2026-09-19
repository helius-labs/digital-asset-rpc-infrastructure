//! Run with DATABASE_TEST_URL set to a disposable local PostgreSQL database:
//! cargo test -p nft_ingester --lib upsert_tests -- --ignored
use super::{token_mint_upsert, upsert_owner_for_account};
use digital_asset_types::dao::tokens;
use sea_orm::{
    ConnectionTrait, Database, DatabaseTransaction, DbBackend, Set, Statement, TransactionTrait,
};

async fn execute(conn: &DatabaseTransaction, sql: &str) {
    conn.execute(Statement::from_string(DbBackend::Postgres, sql.to_owned()))
        .await
        .unwrap();
}

async fn setup() -> DatabaseTransaction {
    let url = std::env::var("DATABASE_TEST_URL").expect("set DATABASE_TEST_URL");
    assert!(matches!(
        url::Url::parse(&url).unwrap().host_str(),
        Some("localhost" | "127.0.0.1")
    ));
    Database::connect(url).await.unwrap().begin().await.unwrap()
}

async fn version(conn: &DatabaseTransaction, table: &str) -> String {
    conn.query_one(Statement::from_string(
        DbBackend::Postgres,
        format!("SELECT ctid::text AS v FROM {table}"),
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "v")
    .unwrap()
}

async fn owner(conn: &DatabaseTransaction, slot: i64) {
    upsert_owner_for_account(
        conn,
        vec![1; 32],
        Some(vec![2; 32]),
        vec![3; 32],
        None,
        slot,
        false,
        None,
        u64::MAX,
        0,
        vec![4; 32],
    )
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires disposable local PostgreSQL via DATABASE_TEST_URL"]
async fn owners_replays_preserve_ordering_and_all_projected_fields() {
    let conn = setup().await;
    execute(
        &conn,
        "CREATE TEMP TABLE owners (
        token_account bytea PRIMARY KEY, owner bytea, mint bytea, delegate bytea,
        slot_updated bigint, frozen boolean, token_extensions jsonb, token_amount bigint,
        token_amount_u64 numeric, delegated_amount bigint, token_program bytea, closed boolean
    ) ON COMMIT DROP",
    )
    .await;
    owner(&conn, 10).await;
    let initial = version(&conn, "owners").await;
    owner(&conn, 10).await;
    assert_eq!(version(&conn, "owners").await, initial);
    // Verify each assigned field participates in the NULL-safe comparison. The
    // production helper must repair a same-slot difference, but never a newer row.
    for change in [
        "owner = NULL",
        "mint = NULL",
        "delegate = decode('01','hex')",
        "frozen = true",
        "token_extensions = '{\"test\":1}'::jsonb",
        "token_amount = 0",
        "token_amount_u64 = 0",
        "delegated_amount = 1",
        "token_program = NULL",
        "closed = true",
    ] {
        execute(&conn, &format!("UPDATE owners SET {change}")).await;
        let changed = version(&conn, "owners").await;
        owner(&conn, 9).await;
        assert_eq!(version(&conn, "owners").await, changed, "stale: {change}");
        owner(&conn, 10).await;
        let repaired = version(&conn, "owners").await;
        assert_ne!(repaired, changed, "same-slot repair: {change}");
        owner(&conn, 10).await;
        assert_eq!(
            version(&conn, "owners").await,
            repaired,
            "duplicate: {change}"
        );
    }
    let before = version(&conn, "owners").await;
    owner(&conn, 11).await;
    let advanced = version(&conn, "owners").await;
    assert_ne!(advanced, before);
    owner(&conn, 10).await;
    assert_eq!(version(&conn, "owners").await, advanced);
    execute(&conn, "UPDATE owners SET slot_updated = NULL").await;
    owner(&conn, 12).await;
    let row = conn
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT slot_updated, token_amount_u64::text AS amount FROM owners".to_owned(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "slot_updated").unwrap(), 12);
    assert_eq!(
        row.try_get::<String>("", "amount").unwrap(),
        u64::MAX.to_string()
    );
    conn.rollback().await.unwrap();
}

fn mint(slot: i64) -> tokens::ActiveModel {
    tokens::ActiveModel {
        mint: Set(vec![1; 32]),
        supply: Set(1),
        decimals: Set(0),
        token_program: Set(vec![2; 32]),
        mint_authority: Set(None),
        freeze_authority: Set(None),
        close_authority: Set(None),
        extension_data: Set(None),
        slot_updated: Set(slot),
        extensions: Set(None),
    }
}

#[tokio::test]
#[ignore = "requires disposable local PostgreSQL via DATABASE_TEST_URL"]
async fn both_token_programs_skip_replays_and_preserve_extension_semantics() {
    let conn = setup().await;
    execute(
        &conn,
        "CREATE TEMP TABLE tokens (
        mint bytea PRIMARY KEY, supply bigint, decimals int, token_program bytea,
        mint_authority bytea, freeze_authority bytea, close_authority bytea,
        extension_data bytea, slot_updated bigint, extensions jsonb
    ) ON COMMIT DROP",
    )
    .await;
    for extension_column in [tokens::Column::ExtensionData, tokens::Column::Extensions] {
        execute(&conn, "TRUNCATE tokens").await;
        conn.execute(token_mint_upsert(mint(10), extension_column))
            .await
            .unwrap();
        let initial = version(&conn, "tokens").await;
        assert_eq!(
            conn.execute(token_mint_upsert(mint(10), extension_column))
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        assert_eq!(version(&conn, "tokens").await, initial);
        let mut changes = vec![
            "supply = 2",
            "decimals = 1",
            "token_program = decode('03','hex')",
            "mint_authority = decode('03','hex')",
            "freeze_authority = decode('03','hex')",
            "close_authority = decode('03','hex')",
        ];
        changes.push(match extension_column {
            tokens::Column::ExtensionData => "extension_data = decode('01','hex')",
            _ => "extensions = '{\"test\":1}'::jsonb",
        });
        for change in changes {
            execute(&conn, &format!("UPDATE tokens SET {change}")).await;
            assert_eq!(
                conn.execute(token_mint_upsert(mint(9), extension_column))
                    .await
                    .unwrap()
                    .rows_affected(),
                0
            );
            assert_eq!(
                conn.execute(token_mint_upsert(mint(10), extension_column))
                    .await
                    .unwrap()
                    .rows_affected(),
                1,
                "{change}"
            );
            assert_eq!(
                conn.execute(token_mint_upsert(mint(10), extension_column))
                    .await
                    .unwrap()
                    .rows_affected(),
                0
            );
        }
        assert_eq!(
            conn.execute(token_mint_upsert(mint(11), extension_column))
                .await
                .unwrap()
                .rows_affected(),
            1
        );
        assert_eq!(
            conn.execute(token_mint_upsert(mint(10), extension_column))
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        // Each path leaves the other extension representation untouched on conflict.
        let unrelated = match extension_column {
            tokens::Column::ExtensionData => "extensions = '{\"unrelated\":1}'::jsonb",
            _ => "extension_data = decode('02','hex')",
        };
        execute(&conn, &format!("UPDATE tokens SET {unrelated}")).await;
        let before = version(&conn, "tokens").await;
        assert_eq!(
            conn.execute(token_mint_upsert(mint(11), extension_column))
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        assert_eq!(version(&conn, "tokens").await, before);
    }
    conn.rollback().await.unwrap();
}
