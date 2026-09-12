use acc_backfill::{fetch_account_data_stream, send_account_stream};
use acc_forwarder::get_token_largest_accounts;
use digital_asset_types::{
    dao::{scopes::asset::get_by_grouping, tokens, PageOptions},
    dapi::common::{create_pagination, create_sorting},
    rpc::{filter::AssetSorting, options::Options},
};
use nft_ingester::config::init_logger;
use sea_orm::{ColumnTrait, Order, QueryOrder, QuerySelect};
use sea_orm::{EntityTrait, QueryFilter, SqlxPostgresConnector};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use {
    acc_forwarder::{
        fetch_and_send_account, fetch_metadata_and_send_accounts, get_token_largest_account,
    },
    anyhow::Context,
    clap::Parser,
    figment::{map, value::Value},
    futures::{future::try_join_all, stream::StreamExt},
    governor::{Quota, RateLimiter},
    log::{info, warn},
    mpl_token_metadata::accounts::{MasterEdition, Metadata},
    plerkle_messenger::{MessengerConfig, ACCOUNT_STREAM},
    solana_client::{
        nonblocking::rpc_client::RpcClient, rpc_config::RpcTransactionConfig,
        rpc_request::RpcRequest,
    },
    solana_commitment_config::{CommitmentConfig, CommitmentLevel},
    solana_sdk::{
        pubkey::Pubkey,
        signature::Signature,
    },
    solana_transaction_status::{
        EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction, UiInstruction, UiMessage,
        UiParsedInstruction, UiTransactionEncoding,
    },
    std::{collections::HashSet, num::NonZeroU32, str::FromStr, sync::Arc},
    tokio::sync::Mutex,
    txn_forwarder::{find_signatures, read_lines, rpc_tx_with_retries},
};

/// Create a rate limiter with the specified requests per second
fn create_rate_limiter(
    requests_per_second: u32,
) -> Arc<
    RateLimiter<
        governor::state::direct::NotKeyed,
        governor::state::InMemoryState,
        governor::clock::DefaultClock,
    >,
> {
    let quota = Quota::per_second(NonZeroU32::new(requests_per_second).unwrap());
    Arc::new(RateLimiter::direct(quota))
}

#[derive(Parser)]
#[command(next_line_help = true)]
struct Args {
    #[arg(long)]
    redis_url: String,
    #[arg(long)]
    rpc_url: String,
    #[arg(long, default_value = "500")]
    max_rpc_calls_per_second: u32,
    #[command(subcommand)]
    action: Action,
}

#[derive(clap::Subcommand, Clone)]
enum Action {
    Account {
        #[arg(long)]
        account: String,
    },
    AccountScenario {
        #[arg(long)]
        scenario_file: String,
    },
    MintScenario {
        #[arg(long)]
        scenario_file: String,
    },
    Mint {
        // puts in mint, token, and metadata account
        #[arg(long)]
        mint: String,
    },
    Token {
        #[arg(long)]
        token: String,
    },
    Collection {
        #[arg(long)]
        collection: String,
        #[arg(long, default_value_t = 25)]
        concurrency: usize,
    },
    CollectionV2 {
        #[arg(long)]
        collection: String,
        #[arg(long)]
        db_url: String,
        #[arg(long, default_value_t = 100)]
        concurrency: usize,
    },
    Token22 {
        #[arg(long)]
        db_url: String,
        #[arg(long, default_value_t = 1000)]
        batch_size: usize,
    },
}

#[derive(Debug)]
struct CollectionTransactionInfo {
    pub program_id: String,
    pub accounts: Vec<String>,
    pub data: String,
}

impl CollectionTransactionInfo {
    fn is_valid(&self) -> bool {
        self.program_id == mpl_token_metadata::ID.to_string()
            && (self.data == "S" || self.data == "K")
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logger();

    let args = Args::parse();
    let config_wrapper = Value::from(map! {
        "redis_connection_str" => args.redis_url.clone(),
        "pipeline_size_bytes" => 1u128.to_string(),
    });
    let config = config_wrapper.into_dict().unwrap();
    let messenger_config = MessengerConfig {
        messenger_type: plerkle_messenger::MessengerType::Redis,
        connection_config: config,
    };
    let mut messenger = plerkle_messenger::select_messenger(messenger_config)
        .await
        .unwrap();
    messenger.add_stream(ACCOUNT_STREAM).await.unwrap();
    messenger
        .set_buffer_size(ACCOUNT_STREAM, 10000000000000000)
        .await;
    let messenger = Arc::new(Mutex::new(messenger));

    let client = RpcClient::new(args.rpc_url.clone());

    match args.action {
        Action::Account { account } => {
            let pubkey = Pubkey::from_str(&account)
                .with_context(|| format!("failed to parse account {account}"))?;
            fetch_and_send_account(pubkey, &client, &messenger, false).await?;
        }
        Action::AccountScenario { scenario_file } => {
            let mut accounts = read_lines(&scenario_file).await?;
            while let Some(maybe_account) = accounts.next().await {
                match maybe_account {
                    Ok(account) => match account.parse::<Pubkey>() {
                        Ok(acc) => {
                            match fetch_and_send_account(acc, &client, &messenger, false).await {
                                Ok(_) => {}
                                Err(e) => {
                                    warn!("Failed to fetch and send account: {:?}", e);
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse account: {:?}", e);
                            continue;
                        }
                    },
                    Err(e) => {
                        warn!("Failed to get next account: {:?}", e);
                        continue;
                    }
                }
            }
        }
        Action::MintScenario { scenario_file } => {
            let mut accounts = read_lines(&scenario_file).await?;
            while let Some(maybe_account) = accounts.next().await {
                match maybe_account {
                    Ok(account) => match account.parse() {
                        Ok(mint) => {
                            let metadata_account = Metadata::find_pda(&mint).0;
                            let token_account = get_token_largest_account(&client, mint).await;

                            match token_account {
                                Ok(token_account) => {
                                    for pubkey in &[mint, metadata_account, token_account] {
                                        match fetch_and_send_account(
                                            *pubkey, &client, &messenger, false,
                                        )
                                        .await
                                        {
                                            Ok(_) => {}
                                            Err(e) => {
                                                warn!("Failed to fetch and send account: {:?}", e);
                                                continue;
                                            }
                                        }
                                    }
                                }
                                Err(e) => warn!("Failed to find mint account: {:?}", e),
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse account: {:?}", e);
                            continue;
                        }
                    },
                    Err(e) => {
                        warn!("Failed to get next account: {:?}", e);
                        continue;
                    }
                }
            }
        }

        Action::Mint { mint } => {
            let mint =
                Pubkey::from_str(&mint).with_context(|| format!("failed to parse mint {mint}"))?;
            let metadata_account = Metadata::find_pda(&mint).0;
            let edition_account = MasterEdition::find_pda(&mint).0;
            let token_account = get_token_largest_account(&client, mint).await;

            match token_account {
                Ok(token_account) => {
                    for pubkey in &[mint, metadata_account, token_account, edition_account] {
                        fetch_and_send_account(*pubkey, &client, &messenger, false).await?;
                    }
                    fetch_and_send_account(edition_account, &client, &messenger, true).await?;
                }
                Err(e) => warn!("Failed to find mint account: {:?}", e),
            }
        }
        Action::Token { token } => {
            let mint = Pubkey::from_str(&token)
                .with_context(|| format!("failed to parse mint {token}"))?;
            let metadata_account = Metadata::find_pda(&mint).0;
            match get_token_largest_accounts(&client, mint).await {
                Ok(token_accounts) => {
                    fetch_and_send_account(mint, &client, &messenger, false).await?;
                    fetch_and_send_account(metadata_account, &client, &messenger, true).await?;
                    let mut all_pubkeys = vec![];
                    all_pubkeys.extend(token_accounts);

                    for pubkey in all_pubkeys {
                        fetch_and_send_account(pubkey, &client, &messenger, false).await?;
                    }
                }
                Err(e) => warn!("Failed to find mint account: {:?}", e),
            }
        }
        Action::Collection {
            collection,
            concurrency,
        } => {
            let metadata_accounts = Arc::new(Mutex::new(HashSet::new()));

            let collection = Pubkey::from_str(&collection)
                .with_context(|| format!("failed to parse collection {collection}"))?;
            let stream = Arc::new(Mutex::new(find_signatures(
                collection, client, None, None, 2_000, false,
            )));

            try_join_all((0..concurrency).map(|_| {
                let metadata_accounts = Arc::clone(&metadata_accounts);
                let stream = Arc::clone(&stream);
                let client = RpcClient::new(args.rpc_url.clone());
                let messenger = Arc::clone(&messenger);
                async move {
                    loop {
                        let mut locked = stream.lock().await;
                        let maybe_signature = locked.recv().await;
                        drop(locked);

                        let mut txinfo = match maybe_signature {
                            Some(signature) => {
                                match collection_get_tx_info(&client, signature?).await? {
                                    Some(txinfo) => txinfo,
                                    None => continue,
                                }
                            }
                            None => return Ok::<(), anyhow::Error>(()),
                        };

                        let account = txinfo.accounts.remove(0);
                        let account = Pubkey::from_str(&account)
                            .with_context(|| format!("failed to parse account {account}"))?;

                        let mut locked = metadata_accounts.lock().await;
                        let inserted = locked.insert(account);
                        drop(locked);

                        if inserted {
                            match fetch_metadata_and_send_accounts(account, &client, &messenger)
                                .await
                            {
                                Ok(_) => info!("Uploaded {:?}", account),
                                Err(e) => warn!("Could not insert {:?}: {:?}", account, e),
                            }
                        }
                    }
                }
            }))
            .await?;
        }
        Action::CollectionV2 {
            collection, db_url, ..
        } => {
            let rate_limiter = create_rate_limiter(args.max_rpc_calls_per_second);
            let pool = setup_database(db_url).await;
            let conn: sea_orm::DatabaseConnection =
                SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
            let limit = 1000;
            let page = 1;
            let pagination_options = PageOptions {
                limit,
                page: Some(page),
                before: None,
                after: None,
                cursor: None,
            };
            let pagination = create_pagination(&pagination_options)?;
            let (sort_direction, sort_column) = create_sorting(AssetSorting::default());

            let (assets, _) = get_by_grouping(
                &conn,
                "collection".to_string(),
                collection,
                sort_column,
                sort_direction,
                &pagination,
                limit,
                false,
                &Options::default(),
            )
            .await?;

            // Real concurrency is determined by rate limiter
            let concurrency = 1000;
            let assets_chunks: Vec<Vec<_>> = assets
                .chunks(assets.len() / concurrency.max(1))
                .map(|chunk| chunk.to_vec())
                .collect();

            try_join_all(assets_chunks.into_iter().map(|chunk| {
                let rate_limiter = Arc::clone(&rate_limiter);
                let client = RpcClient::new(args.rpc_url.clone());
                let messenger = Arc::clone(&messenger);
                async move {
                    for asset in chunk {
                        let asset_id = bs58::encode(asset.asset.id).into_string();
                        info!("re-indexing asset: {}", asset_id);
                        let mint = Pubkey::from_str(&asset_id).unwrap();
                        rate_limiter.until_ready().await;
                        let result = fetch_and_send_account(mint, &client, &messenger, false).await;
                        if let Err(e) = result {
                            warn!("Failed to fetch and send mint account for mint {mint}: {:?}", e);
                        } else {
                            info!("Successfully fetched and sent mint account for mint {mint}");
                        }

                        let metadata_account = Metadata::find_pda(&mint).0;
                        rate_limiter.until_ready().await;
                        // Might not have a metadata account if it's a t22
                        let result =
                            fetch_and_send_account(metadata_account, &client, &messenger, false)
                                .await;
                        if let Err(e) = result {
                            warn!("Failed to fetch and send metadata account for mint {mint} and account {metadata_account}: {:?}", e);
                        } else {
                            info!("Successfully fetched and sent metadata account for mint {mint} and account {metadata_account}");
                        }

                        let token_account = match get_token_largest_account(&client, mint).await {
                            Ok(token_account) => token_account,
                            Err(e) => {
                                warn!("Failed to get token account for mint {mint}: {:?}", e);
                                continue;
                            }
                        };
                        rate_limiter.until_ready().await;
                        // Might not have a token account if it's a metaplex core account
                        let result =
                            fetch_and_send_account(token_account, &client, &messenger, false).await;
                        if let Err(e) = result {
                            warn!("Failed to fetch and send token account for mint {mint} and account {token_account}: {:?}", e);
                        } else {
                            info!("Successfully fetched and sent token account for mint {mint} and account {token_account}");
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                }
            }))
            .await?;
        }
        Action::Token22 { db_url, batch_size } => {
            // Run a script to get all token22 mint accounts
            let pubkey_stream = get_token22_mint_accounts(db_url, batch_size).await;
            let account_stream =
                fetch_account_data_stream(args.rpc_url.clone(), pubkey_stream, true).await;
            send_account_stream(account_stream, args.redis_url.clone()).await;
        }
    }

    Ok(())
}

async fn get_token22_mint_accounts(db_url: String, batch_size: usize) -> Vec<Pubkey> {
    let mut pubkeys = Vec::new();
    let mut cursor: Option<Vec<u8>> = None;
    let pool = match setup_database(db_url).await {
        pool => pool,
    };
    let conn: sea_orm::DatabaseConnection = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
    let token22_program = Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap();

    loop {
        let mut filter = tokens::Column::TokenProgram.eq(token22_program.to_bytes().to_vec());
        if let Some(cursor) = &cursor {
            filter = filter.and(tokens::Column::Mint.lt(cursor.clone()));
        }

        let tokens = match tokens::Entity::find()
            .filter(filter)
            .order_by(tokens::Column::Mint, Order::Desc)
            .limit(batch_size as u64)
            .all(&conn)
            .await
        {
            Ok(tokens) => tokens,
            Err(e) => {
                panic!("Failed to get tokens: {:?}", e);
            }
        };

        if tokens.is_empty() {
            break;
        }

        for token in &tokens {
            let pubkey = Pubkey::try_from(token.mint.as_slice()).unwrap();
            pubkeys.push(pubkey);
        }

        cursor = tokens.last().map(|token| token.mint.clone());
    }
    pubkeys
}

// https://github.com/metaplex-foundation/get-collection/blob/main/get-collection-rs/src/crawl.rs
/// fetch tx and filter
async fn collection_get_tx_info(
    client: &RpcClient,
    signature: Signature,
) -> anyhow::Result<Option<CollectionTransactionInfo>> {
    const CONFIG: RpcTransactionConfig = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::JsonParsed),
        commitment: Some(CommitmentConfig {
            commitment: CommitmentLevel::Finalized,
        }),
        max_supported_transaction_version: Some(u8::MAX),
    };

    let tx: EncodedConfirmedTransactionWithStatusMeta = rpc_tx_with_retries(
        client,
        RpcRequest::GetTransaction,
        serde_json::json!([signature.to_string(), CONFIG]),
        3,
        signature,
    )
    .await?;
    info!("fetch transaction {signature:?}");

    // ignore if tx failed or meta is missed
    let meta = tx.transaction.meta.as_ref();
    if meta.map(|meta| meta.status.is_err()).unwrap_or(true) {
        return Ok(None);
    }

    let tx = match tx.transaction.transaction {
        EncodedTransaction::Json(tx) => tx,
        _ => anyhow::bail!("invalid encoded tx: {signature}"),
    };

    let mut txinfo = None;
    match tx.message {
        UiMessage::Parsed(value) => {
            for ix in value.instructions {
                match ix {
                    UiInstruction::Parsed(ix) => match ix {
                        UiParsedInstruction::PartiallyDecoded(ix) => {
                            txinfo.replace(CollectionTransactionInfo {
                                program_id: ix.program_id,
                                accounts: ix.accounts,
                                data: ix.data,
                            });
                        }
                        // skip system instructions
                        UiParsedInstruction::Parsed(_ix) => {}
                    },
                    UiInstruction::Compiled(ix) => {
                        let accounts: Vec<String> = ix
                            .accounts
                            .chunks(32)
                            .map(|x| bs58::encode(x).into_string())
                            .collect();

                        txinfo.replace(CollectionTransactionInfo {
                            program_id: accounts[ix.program_id_index as usize].clone(),
                            accounts,
                            data: ix.data,
                        });
                    }
                }
            }
        }
        UiMessage::Raw(value) => {
            for ix in value.instructions {
                let accounts: Vec<String> = ix
                    .accounts
                    .chunks(32)
                    .map(|x| bs58::encode(x).into_string())
                    .collect();

                txinfo.replace(CollectionTransactionInfo {
                    program_id: accounts[ix.program_id_index as usize].clone(),
                    accounts,
                    data: ix.data,
                });
            }
        }
    };
    Ok(match txinfo {
        Some(txinfo) if txinfo.is_valid() => Some(txinfo),
        _ => None,
    })
}

pub async fn setup_database(db_url: String) -> PgPool {
    let options: PgConnectOptions = db_url.parse().unwrap();
    let pool = PgPoolOptions::new()
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await
        .unwrap();
    pool
}
