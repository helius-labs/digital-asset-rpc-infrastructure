use std::sync::Arc;

use digital_asset_types::dao::asset;
use nft_ingester::error::IngesterError;
use sea_orm::entity::prelude::*;
use sea_orm::{query::*, DbBackend, EntityTrait, SqlxPostgresConnector};

use log::{error, info};

use nft_ingester::config::init_logger;

use clap::{Arg, ArgAction, Command};

use sqlx::types::chrono::FixedOffset;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions, PgPool,
};

use solana_sdk::bs58;

pub async fn upsert_owner_for_compressed<T>(
    txn: &T,
    id: Vec<u8>,
    owner: Vec<u8>,
    delegate: Option<Vec<u8>>,
    seq: i64,
    created_at: sqlx::types::chrono::DateTime<FixedOffset>,
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
        INSERT INTO "owners" ("mint", "owner", "delegate", "owner_delegate_seq", "created_at")
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT ("owner", "mint") WHERE token_account IS NULL
        DO UPDATE SET
        delegate = CASE
            WHEN excluded.owner_delegate_seq >= owners.owner_delegate_seq OR owners.owner_delegate_seq IS NULL
            THEN EXCLUDED.delegate
            ELSE owners.delegate
        END,
        owner_delegate_seq = CASE
            WHEN excluded.owner_delegate_seq >= owners.owner_delegate_seq OR owners.owner_delegate_seq IS NULL
            THEN EXCLUDED.owner_delegate_seq
            ELSE owners.owner_delegate_seq
        END
        "#,
        vec![
            id.into(),
            owner.into(),
            delegate_value,
            seq.into(),
            created_at.into(),
        ],
    );

    txn.execute(stmt)
        .await
        .map_err(|db_err| IngesterError::AssetIndexError(db_err.to_string()))?;
    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
pub async fn main() {
    init_logger();
    info!("Starting owner migrater creator");

    let matches = Command::new("migrater")
        .arg(
            Arg::new("db-url")
                .long("db-url")
                .short('d')
                .help("Sets the DB url")
                .required(true)
                .action(ArgAction::Set),
        )
        .subcommand(
            Command::new("migrate")
                .about("Migrates a tree from the old schema to the new schema.")
                .arg(
                    Arg::new("tree")
                        .long("tree")
                        .short('t')
                        .help("Address of the merkle tree to migrate.")
                        .required(true)
                        .action(ArgAction::Set),
                ),
        )
        .get_matches();
    let db_url = matches.get_one::<String>("db-url").unwrap();
    let db_url_clone = db_url.clone();
    let pool = get_db_pool(&db_url).await;
    let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
    let tree: Arc<String> = if let Some(migrate_matches) = matches.subcommand_matches("migrate") {
        Arc::new(migrate_matches.get_one::<String>("tree").unwrap().clone())
    } else {
        Arc::new(String::new())
    };

    match matches.subcommand_name() {
        Some("migrate") if !tree.is_empty() => {
            info!("Migrating tree: {}", tree);

            let tree_bytes = bs58::decode(&*tree).into_vec().unwrap();
            let mut offset = 0;
            let mut total_processed = 0;
            let cloned_tree = tree.clone();
            loop {
                let items: Vec<asset::Model> = asset::Entity::find()
                    .filter(asset::Column::TreeId.eq(tree_bytes.clone()))
                    .filter(asset::Column::Owner.is_not_null())
                    .order_by_asc(asset::Column::CreatedAt)
                    .order_by_asc(asset::Column::Id)
                    .limit(100000)
                    .offset(offset)
                    .all(&conn)
                    .await
                    .unwrap();

                let asset_count = items.len();
                if asset_count == 0 {
                    info!(
                        "Done indexing {} asset records (current offset: {}) for tree {}",
                        items.len(),
                        offset,
                        cloned_tree,
                    );
                    break;
                }
                info!(
                    "Fetched {} asset records (current offset: {}) for tree {}",
                    items.len(),
                    offset,
                    cloned_tree
                );
                offset += asset_count as u64;

                let threads = 10;
                let base_chunk_size = asset_count / threads;
                let remainder = asset_count % threads;
                let mut items_processed = 0;

                let mut chunks = vec![];
                for i in 0..threads {
                    let current_chunk_size = if i < remainder {
                        base_chunk_size + 1
                    } else {
                        base_chunk_size
                    };

                    chunks.push(
                        items[items_processed..items_processed + current_chunk_size].to_vec(),
                    );
                    items_processed += current_chunk_size;
                }

                println!("chunks len: {}", chunks.len());
                let handles: Vec<_> = chunks
                    .into_iter()
                    .map(|chunk| {
                        let mut local_counter = 0;
                        let db_url_for_task = db_url_clone.clone();
                        let tree_for_task = tree.clone();
                        tokio::task::spawn(async move {
                            let local_pool = get_db_pool(&db_url_for_task.clone()).await;
                            let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(local_pool);
                            for model in chunk {
                                let result = upsert_owner_for_compressed(
                                    &conn,
                                    model.id,
                                    model.owner.unwrap_or_default(),
                                    Some(model.delegate.unwrap_or_default()),
                                    model.owner_delegate_seq.unwrap_or(0),
                                    model.created_at.unwrap_or_default(),
                                )
                                .await;

                                if let Err(e) = result {
                                    error!("Error while inserting into owners: {:?}", e);
                                }
                                local_counter += 1;
                                if local_counter % 1000 == 0 {
                                    info!(
                                        "Thread {:?}: Indexed {} records for tree {}",
                                        std::thread::current().id(),
                                        local_counter,
                                        tree_for_task,
                                    );
                                }
                            }
                            local_counter
                        })
                    })
                    .collect();

                let results: Vec<_> = futures::future::join_all(handles).await;

                // Handle the results
                let total_processed_in_this_round: usize =
                    results.into_iter().map(|res| res.unwrap_or(0)).sum();
                total_processed += total_processed_in_this_round;
                info!(
                    "Upserted total {} records, loop records {}, (next offset: {}) for tree {}",
                    total_processed, asset_count, offset, tree
                );
            }
        }
        _ => {
            error!("No subcommand provided");
        }
    }
}

async fn get_db_pool(url: &str) -> PgPool {
    let mut options: PgConnectOptions = url.parse().unwrap();
    options.log_statements(log::LevelFilter::Off);
    let pool = PgPoolOptions::new()
        .min_connections(1)
        .max_connections(50)
        .connect_with(options)
        .await
        .unwrap();
    pool
}
