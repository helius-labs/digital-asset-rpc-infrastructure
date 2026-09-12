use crate::{error::IngesterError, metric};
use cadence_macros::is_global_default_set;
use cadence_macros::statsd_count;
use common::constant::FAKE_SOL_PUBKEY;
use digital_asset_types::dao::price;
use rand::Rng;
use sea_orm::FromQueryResult;
use sea_orm::QuerySelect;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use log::{error, info};
use sea_orm::{
    sea_query::OnConflict, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, QueryTrait,
    Set, SqlxPostgresConnector,
};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres};
use tokio::{task::JoinHandle, time};

const LOOP_INTERVAL_SECONDS: u64 = 60_000;

pub struct PriceTaskManager {
    pool: Pool<Postgres>,
}

pub async fn update_sol_token_price(
    db: Arc<DatabaseConnection>,
    sol_price: f64,
) -> Result<(), IngesterError> {
    let mint_bytes = bs58::decode(FAKE_SOL_PUBKEY)
        .into_vec()
        .map_err(|_| IngesterError::SerializatonError("Invalid 'id' format".to_string()))?;

    let price_model: price::ActiveModel = price::ActiveModel {
        mint: Set(mint_bytes),
        price: Set(Some(sol_price as f32)),
        symbol: Set(Some("SOL".to_string())),
    };

    price::Entity::insert(price_model)
        .on_conflict(
            OnConflict::columns([price::Column::Mint])
                .update_columns([price::Column::Price])
                .to_owned(),
        )
        .exec(db.as_ref())
        .await
        .map_err(|e| IngesterError::DatabaseError(e.to_string()))?;

    info!("Updated SOL price");

    Ok(())
}

fn contains_null_byte(symbol: &Option<String>) -> bool {
    matches!(symbol, Some(s) if s.as_bytes().contains(&0x00))
}

impl PriceTaskManager {
    pub fn new(pool: Pool<Postgres>) -> Self {
        PriceTaskManager { pool }
    }

    async fn update_token_prices(
        tokens_chunk: Vec<TokenPrice>,
        db: Arc<DatabaseConnection>,
    ) -> Result<(), IngesterError> {
        info!("Updating prices for {} tokens", tokens_chunk.len());
        for token_price_data in tokens_chunk.iter() {
            if contains_null_byte(&token_price_data.symbol) {
                continue;
            }

            let mint = token_price_data.address.clone();
            let mint_bytes = bs58::decode(&mint).into_vec().map_err(|_| {
                IngesterError::SerializatonError(format!("Invalid 'id' format: {}", mint))
            })?;

            let price_model: price::ActiveModel = price::ActiveModel {
                mint: Set(mint_bytes),
                price: Set(Some(token_price_data.price.unwrap() as f32)),
                symbol: Set(token_price_data.symbol.clone()),
            };

            let query = price::Entity::insert(price_model)
                .on_conflict(
                    OnConflict::columns([price::Column::Mint])
                        .update_columns([price::Column::Price, price::Column::Symbol])
                        .to_owned(),
                )
                .build(DbBackend::Postgres);

            db.execute(query)
                .await
                .map_err(|e| IngesterError::DatabaseError(e.to_string()))?;
        }
        Ok(())
    }

    pub fn start_runner(self: Arc<Self>, loop_interval_seconds: Option<u64>) -> JoinHandle<()> {
        let loop_interval = loop_interval_seconds.unwrap_or(LOOP_INTERVAL_SECONDS);

        let conn = Arc::new(SqlxPostgresConnector::from_sqlx_postgres_pool(
            self.pool.clone(),
        ));

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(loop_interval));
            loop {
                interval.tick().await;

                match update_pricing_table(conn.clone()).await {
                    Ok(_) => {
                        metric! {
                            statsd_count!("ingester.price_manager.token_price_fetching_success", 1);
                        }
                    }
                    Err(e) => {
                        error!("Error executing task: {}", e);
                    }
                }
            }
        })
    }
}

async fn delete_removed_tokens(
    db: Arc<DatabaseConnection>,
    tokens: Vec<TokenPrice>,
) -> Result<(), IngesterError> {
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, FromQueryResult, Default)]
    struct MintRow {
        mint: Vec<u8>,
    }

    let existing_tokens: Vec<String> = price::Entity::find()
        .select_only()
        .column(price::Column::Mint)
        .into_model::<MintRow>()
        .all(db.as_ref())
        .await
        .map_err(|e| IngesterError::DatabaseError(e.to_string()))?
        .into_iter()
        .map(|x| bs58::encode(x.mint).into_string())
        .collect();
    let addresses = tokens
        .iter()
        .map(|token| token.address.clone())
        .collect::<HashSet<String>>();
    let mut price_tokens_set: HashSet<String> = HashSet::from_iter(addresses);
    price_tokens_set.insert(FAKE_SOL_PUBKEY.to_string());

    let tokens_to_delete = existing_tokens
        .into_iter()
        .filter(|x| !price_tokens_set.contains(x))
        .map(|x| bs58::decode(x).into_vec().unwrap())
        .collect::<Vec<Vec<u8>>>();

    for token in tokens_to_delete.iter() {
        info!("Deleting token: {}", bs58::encode(token).into_string());
        price::Entity::delete_by_id(token.clone())
            .exec(db.as_ref())
            .await
            .map_err(|e| IngesterError::DatabaseError(e.to_string()))?;
    }

    Ok(())
}

pub async fn update_pricing_table(db: Arc<DatabaseConnection>) -> Result<(), IngesterError> {
    let tokens = load_all_birdeye_tokens().await?;
    let sol_price = tokens
        .iter()
        .find(|token| token.address == "So11111111111111111111111111111111111111112")
        .unwrap()
        .price
        .unwrap();
    update_sol_token_price(db.clone(), sol_price).await?;
    PriceTaskManager::update_token_prices(tokens.clone(), db.clone()).await?;
    delete_removed_tokens(db.clone(), tokens.clone()).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPrice {
    pub symbol: Option<String>,
    pub price: Option<f64>,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPriceResponse {
    pub data: TokenPriceDataBirdeye,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPriceDataBirdeye {
    pub items: Vec<TokenPrice>,
    pub has_next: bool,
}

async fn load_tokens_birdeye_with_descending_24volume(
    limit: usize,
    offset: usize,
) -> Result<Vec<TokenPrice>, IngesterError> {
    let url = format!("https://public-api.birdeye.so/defi/v3/token/list?sort_by=volume_24h_usd&sort_type=desc&offset={}&limit={}&ui_amount_mode=scaled", offset, limit);
    let client = reqwest::Client::new();
    let max_retries = 20;
    let mut num_retries = 0;

    loop {
        let response = client
            .get(&url)
            .header("X-API-KEY", std::env::var("BIRDEYE_API_KEY").expect("BIRDEYE_API_KEY"))
            .header("accept", "application/json")
            .header("x-chain", "solana")
            .send()
            .await?;

        if response.status().is_success() {
            let parsed_response = response.json::<TokenPriceResponse>().await;

            return Ok(parsed_response?
                .data
                .items
                .into_iter()
                .filter(|item| item.price.is_some())
                .collect());
        }
        error!("Error fetching tokens from Birdeye: {}", response.status());
        let rand_sleep = rand::rng().random_range(1..=10);
        if num_retries > max_retries {
            return Err(IngesterError::HttpError {
                status_code: response.status().to_string(),
                uri: url.clone(),
            });
        }
        num_retries += 1;

        tokio::time::sleep(std::time::Duration::from_secs(rand_sleep)).await;
    }
}

async fn load_all_birdeye_tokens() -> Result<Vec<TokenPrice>, IngesterError> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    let limit = 100;
    let max_num_pages = 100;
    while offset < max_num_pages * limit {
        let new_tokens = load_tokens_birdeye_with_descending_24volume(limit, offset).await?;
        tokens.extend(new_tokens);
        offset += limit;
    }
    Ok(tokens)
}

#[tokio::test]
#[ignore]
async fn test_load_all_tokens() {
    let tokens = load_all_birdeye_tokens().await.unwrap();
    assert!(tokens.len() > 0);
}

#[tokio::test]
#[ignore]
async fn test_update_pricing_table() {
    use common::db::setup_pg_pool;

    let pool =
        setup_pg_pool("postgresql://postgres:postgres@localhost:5432/postgres".to_string()).await;
    let db = SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone());
    update_pricing_table(Arc::new(db)).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DatabaseBackend, DatabaseConnection, MockDatabase, MockExecResult};

    #[tokio::test]
    async fn test_update_token_prices_skips_tokens_with_null_bytes() {
        let db = create_mock_db(1);

        let tokens = vec![
            token_with_null_byte(), // Should be skipped
            valid_sol_token(),       // Should be inserted
        ];

        let result = PriceTaskManager::update_token_prices(tokens, Arc::new(db)).await;

        // Should succeed, having skipped the invalid token
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_token_prices_allows_control_chars() {
        let db = create_mock_db(2);

        let tokens = vec![
            token_with_control_chars(), // Control chars are allowed, should be inserted
            valid_usdc_token(),          // Should be inserted
        ];

        let result = PriceTaskManager::update_token_prices(tokens, Arc::new(db)).await;

        // Should succeed with both tokens inserted (PostgreSQL allows control chars)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_token_prices_handles_all_valid_tokens() {
        let db = create_mock_db(2);

        let tokens = vec![
            valid_sol_token(),
            valid_usdc_token(),
        ];

        let result = PriceTaskManager::update_token_prices(tokens, Arc::new(db)).await;

        // Should succeed with both tokens inserted
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_token_prices_skips_all_tokens_with_null_bytes() {
        let db = create_mock_db(0);

        let tokens = vec![
            token_with_null_byte(),
            create_test_token(
                "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
                Some("USDT\0INVALID".to_string()),
                1.0,
            ),
        ];

        let result = PriceTaskManager::update_token_prices(tokens, Arc::new(db)).await;

        // Should succeed without inserting anything (both have null bytes)
        assert!(result.is_ok());
    }

    fn create_test_token(address: &str, symbol: Option<String>, price: f64) -> TokenPrice {
        TokenPrice {
            address: address.to_string(),
            symbol,
            price: Some(price),
        }
    }

    fn token_with_null_byte() -> TokenPrice {
        create_test_token(
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            Some("USDC\0BAD".to_string()),
            1.0,
        )
    }

    fn token_with_control_chars() -> TokenPrice {
        create_test_token(
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
            Some("USDT\x01\x02".to_string()),
            1.0,
        )
    }

    fn valid_sol_token() -> TokenPrice {
        create_test_token(
            "So11111111111111111111111111111111111111112",
            Some("SOL".to_string()),
            100.0,
        )
    }

    fn valid_usdc_token() -> TokenPrice {
        create_test_token(
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            Some("USDC".to_string()),
            1.0,
        )
    }

    fn create_mock_db(expected_inserts: usize) -> DatabaseConnection {
        let mut mock_db = MockDatabase::new(DatabaseBackend::Postgres);

        for _ in 0..expected_inserts {
            mock_db = mock_db.append_exec_results(vec![MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }]);
        }

        mock_db.into_connection()
    }
}
