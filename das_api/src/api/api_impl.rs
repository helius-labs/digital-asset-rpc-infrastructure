use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use crate::{
    feature_flag::get_feature_flags,
    validation::{
        validate_opt_pubkey, validate_pubkey, validate_search_assets_query,
        validate_search_with_name, RequestValidator,
    },
};
use common::constant::FAKE_SOL_PUBKEY;
use digital_asset_types::{
    dao::{
        price,
        sea_orm_active_enums::{
            OwnerType, RoyaltyTargetType, SpecificationAssetClass, SpecificationVersions,
        },
        Cursor, NotFilter, PageOptions, SearchAssetsQuery,
    },
    dapi::{
        get_asset, get_asset_list, get_asset_proof, get_asset_proofs, get_asset_signatures,
        get_assets_by_authority, get_assets_by_creator, get_assets_by_group, get_assets_by_owner,
        get_nft_editions, get_token_accounts, search_assets, search_owners, search_tokens,
    },
    feature_flag::FeatureFlags,
    rpc::{
        filter::{AssetSortBy, SearchConditionType, TokenSortBy, TokenSorting},
        response::{GetAssetsV2Response, NativeBalance},
        OwnershipModel, RoyaltyModel,
    },
};
use log::warn;
use open_rpc_derive::document_rpc;
use open_rpc_schema::document::OpenrpcDocument;
use sea_orm::ColumnTrait;
use sea_orm::{
    sea_query::ConditionType, ConnectionTrait, DbBackend, EntityTrait, QueryFilter, Statement,
};
use serde_json::{json, Value};
use solana_account_decoder::UiAccountEncoding;
use solana_client::{
    nonblocking::rpc_client::RpcClient, rpc_config::RpcAccountInfoConfig, rpc_request::RpcRequest,
};
use solana_rpc_client_api::client_error::Error;
use solana_commitment_config::{CommitmentConfig, CommitmentLevel};
use solana_sdk::pubkey::Pubkey;
use sqlx::{postgres::PgPoolOptions, Executor};
use {
    crate::api::*,
    crate::config::Config,
    crate::error::DasApiError,
    async_trait::async_trait,
    digital_asset_types::rpc::{
        response::AssetList, response::TransactionSignatureList, Asset, AssetProof,
    },
    sea_orm::{DatabaseConnection, DbErr, SqlxPostgresConnector},
};

pub struct LoadBalancer {
    pools: Vec<Arc<DatabaseConnection>>,
    current_index: AtomicUsize,
}

impl LoadBalancer {
    pub fn new(pools: Vec<Arc<DatabaseConnection>>) -> Self {
        LoadBalancer {
            pools,
            current_index: AtomicUsize::new(0),
        }
    }

    pub fn next(&self) -> Arc<DatabaseConnection> {
        let index = self.current_index.fetch_add(1, Ordering::Relaxed) % self.pools.len();
        self.pools[index].clone()
    }
}

pub struct DasApi {
    db_lb: LoadBalancer,
    cdn_prefix: Option<String>,
    feature_flags: FeatureFlags,
    validator: RequestValidator,
    pub rpc_client: Arc<RpcClient>,
}

async fn create_pool(
    db_url: &str,
    max_conn: Option<u32>,
    work_mem: Option<String>,
) -> Result<DatabaseConnection, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(max_conn.unwrap_or(100))
        .after_connect(move |conn, _meta| {
            let work_mem = work_mem.clone();
            Box::pin(async move {
                conn.execute("SET statement_timeout = '15s'").await?;
                if let Some(wm) = &work_mem {
                    conn.execute(format!("SET work_mem = '{}'", wm).as_str())
                        .await?;
                }
                Ok(())
            })
        })
        .connect(db_url).await?;
    let conn = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
    return Ok(conn);
}

impl DasApi {
    pub async fn from_config(config: Config) -> Result<Self, DasApiError> {
        let work_mem = config.db_work_mem.clone();
        let mut pools = Vec::new();
        let feature_flags = get_feature_flags(&config);
        let validator = RequestValidator::from_config(config.clone());
        let rpc_client = Arc::new(RpcClient::new(config.rpc_url.clone()));
        let db_urls = validator.get_database_urls();
        if !db_urls.is_empty() {
            for db_url in db_urls {
                let pool = create_pool(&db_url, config.db_max_conn, work_mem.clone()).await?;
                pools.push(Arc::new(pool));
            }
        } else {
            return Err(DasApiError::ConfigurationError(
                "No database urls provided".to_string(),
            ));
        }

        let db_lb = LoadBalancer::new(pools);

        Ok(DasApi {
            rpc_client,
            db_lb,
            cdn_prefix: config.cdn_prefix,
            feature_flags,
            validator,
        })
    }

    fn get_cursor(&self, cursor: &Option<String>) -> Result<Cursor, DasApiError> {
        match cursor {
            Some(cursor_b64) => {
                let cursor_vec = bs58::decode(cursor_b64)
                    .into_vec()
                    .map_err(|_| DasApiError::CursorValidationError(cursor_b64.clone()))?;
                let cursor_struct = Cursor {
                    id: Some(cursor_vec),
                };
                Ok(cursor_struct)
            }
            None => Ok(Cursor::default()),
        }
    }

    fn validate_pagination(
        &self,
        limit: &Option<u32>,
        page: &Option<u32>,
        before: &Option<String>,
        after: &Option<String>,
        cursor: &Option<String>,
        sorting: &Option<&AssetSorting>,
    ) -> Result<PageOptions, DasApiError> {
        let mut is_cursor_enabled = true;
        let mut page_opt = PageOptions::default();

        if let Some(limit) = limit {
            // make config item
            if *limit > 1000 {
                return Err(DasApiError::PaginationExceededError);
            }
        }

        if let Some(page) = page {
            if *page == 0 {
                return Err(DasApiError::PaginationEmptyError);
            }

            // make config item
            if before.is_some() || after.is_some() || cursor.is_some() {
                return Err(DasApiError::PaginationError);
            }

            let current_limit = limit.unwrap_or(1000);
            let offset = (*page - 1) * current_limit;
            if offset > 500_000 {
                return Err(DasApiError::OffsetLimitExceededError);
            }
            is_cursor_enabled = false;
        }

        if let Some(before) = before {
            if cursor.is_some() {
                return Err(DasApiError::PaginationError);
            }
            if let Some(sort) = &sorting {
                if sort.sort_by != AssetSortBy::Id {
                    return Err(DasApiError::PaginationSortingValidationError);
                }
            }
            validate_pubkey(before.clone())?;
            is_cursor_enabled = false;
        }

        if let Some(after) = after {
            if cursor.is_some() {
                return Err(DasApiError::PaginationError);
            }
            if let Some(sort) = &sorting {
                if sort.sort_by != AssetSortBy::Id {
                    return Err(DasApiError::PaginationSortingValidationError);
                }
            }
            validate_pubkey(after.clone())?;
            is_cursor_enabled = false;
        }

        page_opt.limit = limit.map(|x| x as u64).unwrap_or(1000);
        if is_cursor_enabled {
            if let Some(sort) = &sorting {
                if sort.sort_by != AssetSortBy::Id {
                    return Err(DasApiError::PaginationSortingValidationError);
                }
                page_opt.cursor = Some(self.get_cursor(cursor)?);
            }
        } else {
            page_opt.page = page.map(|x| x as u64);
            page_opt.before = before
                .clone()
                .map(|x| bs58::decode(x).into_vec().unwrap_or_default());
            page_opt.after = after
                .clone()
                .map(|x| bs58::decode(x).into_vec().unwrap_or_default());
        }
        Ok(page_opt)
    }

    fn validate_token_pagination(
        &self,
        limit: &Option<u32>,
        page: &Option<u32>,
        before: &Option<String>,
        after: &Option<String>,
        cursor: &Option<String>,
    ) -> Result<PageOptions, DasApiError> {
        let mut is_cursor_enabled = true;
        let mut page_opt = PageOptions::default();

        if let Some(limit) = limit {
            // make config item
            if *limit > 1000 {
                return Err(DasApiError::PaginationExceededError);
            }
        }

        if let Some(page) = page {
            if *page == 0 {
                return Err(DasApiError::PaginationEmptyError);
            }

            // make config item
            if before.is_some() || after.is_some() || cursor.is_some() {
                return Err(DasApiError::PaginationError);
            }

            let current_limit = limit.unwrap_or(1000);
            let offset = (*page - 1) * current_limit;
            if offset > 500_000 {
                return Err(DasApiError::OffsetLimitExceededError);
            }
            is_cursor_enabled = false;
        }

        if let Some(before) = before {
            if cursor.is_some() {
                return Err(DasApiError::PaginationError);
            }
            validate_pubkey(before.clone())?;
            is_cursor_enabled = false;
        }

        if let Some(after) = after {
            if cursor.is_some() {
                return Err(DasApiError::PaginationError);
            }
            validate_pubkey(after.clone())?;
            is_cursor_enabled = false;
        }

        page_opt.limit = limit.map(|x| x as u64).unwrap_or(1000);
        if is_cursor_enabled {
            page_opt.cursor = Some(self.get_cursor(cursor)?);
        } else {
            page_opt.page = page.map(|x| x as u64);
            page_opt.before = before
                .clone()
                .map(|x| bs58::decode(x).into_vec().unwrap_or_default());
            page_opt.after = after
                .clone()
                .map(|x| bs58::decode(x).into_vec().unwrap_or_default());
        }
        Ok(page_opt)
    }
}

pub fn not_found(asset_id: &String) -> DbErr {
    DbErr::RecordNotFound(format!("Asset Proof for {} Not Found", asset_id))
}

#[document_rpc]
#[async_trait]
impl ApiContract for DasApi {
    // Legacy health check
    async fn check_health(self: &DasApi) -> Result<(), DasApiError> {
        self.db_lb
            .next()
            .execute(Statement::from_string(
                DbBackend::Postgres,
                "SELECT 1".to_string(),
            ))
            .await?;
        Ok(())
    }

    // Liveness probe determines if the pod is healthy. Kubernetes will restart the pod if this fails.
    async fn liveness(self: &DasApi) -> Result<(), DasApiError> {
        Ok(())
    }

    // Readiness probe determines if the pod has capacity to accept traffic. Kubernetes will not route traffic to this pod if this fails.
    // We are essentially checking if there are DB connections available.
    async fn readiness(self: &DasApi) -> Result<(), DasApiError> {
        self.db_lb
            .next()
            .execute(Statement::from_string(
                DbBackend::Postgres,
                "SELECT 1".to_string(),
            ))
            .await?;
        Ok(())
    }

    async fn get_asset_proof(
        self: &DasApi,
        payload: GetAssetProof,
    ) -> Result<AssetProof, DasApiError> {
        let id = validate_pubkey(payload.id.clone())?;
        let id_bytes = id.to_bytes().to_vec();
        let result = get_asset_proof(&self.db_lb.next(), id_bytes).await;
        if let Err(ref e) = result {
            log::error!("get_asset_proof failed: id={} error={}", payload.id, e);
        }
        result
            .and_then(|p| {
                if p.proof.is_empty() {
                    log::error!("get_asset_proof empty proof: id={}", payload.id);
                    return Err(not_found(&payload.id));
                }
                Ok(p)
            })
            .map_err(Into::into)
    }

    async fn get_asset_proofs(
        self: &DasApi,
        payload: GetAssetProofs,
    ) -> Result<HashMap<String, Option<AssetProof>>, DasApiError> {
        let GetAssetProofs { ids } = payload;

        let batch_size = ids.len();
        if batch_size > 1000 {
            return Err(DasApiError::BatchSizeExceededError);
        }

        let id_bytes = ids
            .iter()
            .map(|id| validate_pubkey(id.clone()).map(|id| id.to_bytes().to_vec()))
            .collect::<Result<Vec<Vec<u8>>, _>>()?;

        let proofs = get_asset_proofs(&self.db_lb.next(), id_bytes).await?;

        let result: HashMap<String, Option<AssetProof>> = ids
            .iter()
            .map(|id| (id.clone(), proofs.get(id).cloned()))
            .collect();
        Ok(result)
    }

    async fn get_asset(self: &DasApi, payload: GetAsset) -> Result<Asset, DasApiError> {
        let GetAsset {
            id,
            raw_data, // TODO: Deprecate
            options,
        } = payload;
        let id_bytes = validate_pubkey(id.clone())?.to_bytes().to_vec();
        let mut options = options.unwrap_or_default();
        options.cdn_prefix = self.cdn_prefix.clone();

        // TODO: Remove once no one is using.
        if let Some(rd) = raw_data {
            options.show_raw_data = rd;
        }

        let asset = get_asset(
            &self.db_lb.next(),
            id_bytes,
            &self.feature_flags,
            &options.into(),
        )
        .await;

        match asset {
            Ok(asset) => Ok(asset),
            Err(e) => {
                // Every 1 in 100 requests, print out the error
                if rand::random::<u8>() % 100 == 0 {
                    println!("Error getting asset: {} {:?}", id, e);
                }
                Err(DasApiError::from(e))
            }
        }
    }

    async fn get_assets(
        self: &DasApi,
        payload: GetAssets,
    ) -> Result<Vec<Option<Asset>>, DasApiError> {
        let result = self.get_assets_v2(payload).await?;
        Ok(result.items)
    }

    async fn get_assets_v2(
        self: &DasApi,
        payload: GetAssets,
    ) -> Result<GetAssetsV2Response, DasApiError> {
        let GetAssets { ids, options } = payload;
        let batch_size = ids.len();
        if batch_size > 1000 {
            return Err(DasApiError::BatchSizeExceededError);
        }

        let id_bytes = ids
            .iter()
            .map(|id| validate_pubkey(id.clone()).map(|id| id.to_bytes().to_vec()))
            .collect::<Result<Vec<Vec<u8>>, _>>()?;

        let mut options = options.unwrap_or_default();
        options.cdn_prefix = self.cdn_prefix.clone();

        let asset_list = get_asset_list(
            &self.db_lb.next(),
            id_bytes,
            batch_size as u64,
            &self.feature_flags,
            &options.into(),
        )
        .await
        .map_err(DasApiError::from)?;
        let asset_map: HashMap<String, Asset> = asset_list
            .items
            .into_iter()
            .map(|asset| (asset.id.clone(), asset))
            .collect();
        let items = ids.iter().map(|id| asset_map.get(id).cloned()).collect();
        Ok(GetAssetsV2Response {
            last_indexed_slot: asset_list.last_indexed_slot,
            items,
        })
    }

    async fn get_assets_by_owner(
        self: &DasApi,
        payload: GetAssetsByOwner,
    ) -> Result<AssetList, DasApiError> {
        let GetAssetsByOwner {
            owner_address,
            sort_by,
            limit,
            page,
            before,
            after,
            options,
            cursor,
        } = payload;
        let before: Option<String> = before.filter(|before| !before.is_empty());
        let after: Option<String> = after.filter(|after| !after.is_empty());
        let owner_address = validate_pubkey(owner_address.clone())?;
        let owner_address_bytes = owner_address.to_bytes().to_vec();
        let sort_by = sort_by.unwrap_or_default();
        let page_options =
            self.validate_pagination(&limit, &page, &before, &after, &cursor, &Some(&sort_by))?;
        let mut options = options.unwrap_or_default();
        options.cdn_prefix = self.cdn_prefix.clone();
        // Similarly get native balance here
        let owner_pubkey = Pubkey::try_from(owner_address_bytes.clone())
            .map_err(|_| DasApiError::ValidationError("Invalid owner address".to_string()))?;

        let show_native_balance = options.show_native_balance;

        let (native_balance, result) = tokio::join!(
            async {
                let db = &self.db_lb.next();
                optionally_fetch_native_balance(
                    db,
                    show_native_balance,
                    Some(owner_pubkey),
                    &self.rpc_client,
                )
                .await
            },
            async {
                get_assets_by_owner(
                    &self.db_lb.next(),
                    owner_address_bytes.clone(),
                    sort_by,
                    &page_options,
                    &self.feature_flags,
                    &options,
                    None,
                    None,
                    None,
                )
                .await
            }
        );
        let native_balance = native_balance?;
        let mut result = result.map_err(DasApiError::from)?;
        result.nativeBalance = native_balance;
        Ok(result)
    }

    async fn get_assets_by_group(
        self: &DasApi,
        payload: GetAssetsByGroup,
    ) -> Result<AssetList, DasApiError> {
        let GetAssetsByGroup {
            group_key,
            group_value,
            sort_by,
            limit,
            page,
            before,
            after,
            options,
            cursor,
        } = payload;
        self.validator
            .validate_options(&group_key, &group_value, &options)?;
        validate_pubkey(group_value.clone())?;
        let sort_by = sort_by.unwrap_or_default();
        let before: Option<String> = before.filter(|before| !before.is_empty());
        let after: Option<String> = after.filter(|after| !after.is_empty());
        let page_options =
            self.validate_pagination(&limit, &page, &before, &after, &cursor, &Some(&sort_by))?;
        let mut options = options.unwrap_or_default();
        options.cdn_prefix = self.cdn_prefix.clone();
        get_assets_by_group(
            &self.db_lb.next(),
            group_key,
            group_value,
            sort_by,
            &page_options,
            &self.feature_flags,
            &options,
        )
        .await
        .map_err(Into::into)
    }

    async fn get_assets_by_creator(
        self: &DasApi,
        payload: GetAssetsByCreator,
    ) -> Result<AssetList, DasApiError> {
        let GetAssetsByCreator {
            creator_address,
            only_verified,
            sort_by,
            limit,
            page,
            before,
            after,
            options,
            cursor,
        } = payload;
        let search_key = String::from("creators");
        self.validator
            .validate_options(&search_key, &creator_address, &options)?;
        let creator_address = validate_pubkey(creator_address.clone())?;
        let creator_address_bytes = creator_address.to_bytes().to_vec();

        let sort_by = sort_by.unwrap_or_default();
        let page_options =
            self.validate_pagination(&limit, &page, &before, &after, &cursor, &Some(&sort_by))?;
        let only_verified = only_verified.unwrap_or_default();
        let mut options = options.unwrap_or_default();
        options.cdn_prefix = self.cdn_prefix.clone();
        get_assets_by_creator(
            &self.db_lb.next(),
            creator_address_bytes,
            only_verified,
            sort_by,
            &page_options,
            &self.feature_flags,
            &options,
        )
        .await
        .map_err(Into::into)
    }

    async fn get_assets_by_authority(
        self: &DasApi,
        payload: GetAssetsByAuthority,
    ) -> Result<AssetList, DasApiError> {
        let GetAssetsByAuthority {
            authority_address,
            sort_by,
            limit,
            page,
            before,
            after,
            options,
            cursor,
        } = payload;
        let search_key = String::from("authority");
        self.validator
            .validate_options(&search_key, &authority_address, &options)?;
        let sort_by = sort_by.unwrap_or_default();
        let authority_address = validate_pubkey(authority_address.clone())?;
        let authority_address_bytes = authority_address.to_bytes().to_vec();
        let page_options =
            self.validate_pagination(&limit, &page, &before, &after, &cursor, &Some(&sort_by))?;
        let mut options = options.unwrap_or_default();
        options.cdn_prefix = self.cdn_prefix.clone();
        get_assets_by_authority(
            &self.db_lb.next(),
            authority_address_bytes,
            sort_by,
            &page_options,
            &self.feature_flags,
            &options,
        )
        .await
        .map_err(Into::into)
    }

    async fn search_assets(&self, payload: SearchAssets) -> Result<AssetList, DasApiError> {
        let SearchAssets {
            negate,
            not,
            condition_type,
            interface,
            owner_address,
            owner_type,
            creator_address,
            creator_verified,
            authority_address,
            grouping,
            delegate,
            frozen,
            supply,
            supply_mint,
            compressed,
            compressible,
            royalty_target_type,
            royalty_target,
            royalty_amount,
            burnt,
            sort_by,
            limit,
            page,
            before,
            after,
            json_uri,
            options,
            cursor,
            name,
            collections,
            token_type,
            created_at,
            tree,
            collection_nft,
            is_agent,
            agent_token,
            asset_signer,
        } = payload;
        // Drip wants to query across multiple collections given a single owner address
        if let Some(collections) = collections.clone() {
            if owner_address.is_none() || grouping.is_some() {
                return Err(DasApiError::ValidationError(
                    "Must provide `owner_address` and not use `grouping` when using `collections` field".to_string(),
                ));
            }
            for str_pubkey in collections {
                validate_pubkey(str_pubkey.to_string())?;
            }
        }

        if let Some(grouping) = grouping.clone() {
            validate_pubkey(grouping.1.clone())?;
        }

        if let Some(_collections_nft) = collection_nft {
            if owner_address.is_none()
                && grouping.is_none()
                && creator_address.is_none()
                && authority_address.is_none()
            {
                return Err(DasApiError::ValidationError(
                    "Must provide either `owner_address` or `grouping` or `creator_address` or `authority_address` when using `collection_nft` field".to_string(),
                ));
            }
        }

        // Validate options. If owner_address is provided, we'll allow big collections.
        if owner_address.is_none() {
            if let Some((k, v)) = &grouping {
                self.validator
                    .validate_options(k, v, &options.clone().map(|o| o.into()))?;
            }
            if let Some(c) = &creator_address {
                self.validator.validate_options(
                    &String::from("creators"),
                    c,
                    &options.clone().map(|o| o.into()),
                )?;
            }
            if let Some(a) = &authority_address {
                self.validator.validate_options(
                    &String::from("authority"),
                    a,
                    &options.clone().map(|o| o.into()),
                )?;
            }
        }

        let spec: Option<(SpecificationVersions, SpecificationAssetClass)> =
            interface.map(|x| x.into());
        let specification_version = spec.clone().map(|x| x.0);
        let specification_asset_class = spec.map(|x| x.1);
        let condition_type = condition_type.map(|x| match x {
            SearchConditionType::Any => ConditionType::Any,
            SearchConditionType::All => ConditionType::All,
        });
        let owner_address = validate_opt_pubkey(&owner_address)?;
        let name = validate_search_with_name(&name, &owner_address)?;
        let creator_address = validate_opt_pubkey(&creator_address)?;
        let delegate = validate_opt_pubkey(&delegate)?;

        let authority_address = validate_opt_pubkey(&authority_address)?;
        let supply_mint = validate_opt_pubkey(&supply_mint)?;
        let royalty_target = validate_opt_pubkey(&royalty_target)?;
        let tree = validate_opt_pubkey(&tree)?;

        let agent_token = validate_opt_pubkey(&agent_token)?;
        let asset_signer = validate_opt_pubkey(&asset_signer)?;

        let owner_type = owner_type.map(|x| match x {
            OwnershipModel::Single => OwnerType::Single,
            OwnershipModel::Token => OwnerType::Token,
        });

        let royalty_target_type = royalty_target_type.map(|x| match x {
            RoyaltyModel::Creators => RoyaltyTargetType::Creators,
            RoyaltyModel::Fanout => RoyaltyTargetType::Fanout,
            RoyaltyModel::Single => RoyaltyTargetType::Single,
        });

        let mut not_filter = None;
        if let Some(not) = not {
            let owners = convert_strings_to_bytes(not.owners)?;
            let creators = convert_strings_to_bytes(not.creators)?;
            let authorities = convert_strings_to_bytes(not.authorities)?;
            let collections = match not.collections {
                None => None,
                Some(groups) => {
                    let ans = groups
                        .into_iter()
                        .map(|g| validate_pubkey(g.clone()).map(|_| g))
                        .collect::<Result<Vec<String>, _>>()?;
                    if ans.is_empty() {
                        None
                    } else {
                        Some(ans)
                    }
                }
            };
            not_filter = Some(NotFilter {
                collections,
                owners,
                creators,
                authorities,
            })
        }

        let saq = SearchAssetsQuery {
            negate,
            not_filter,
            condition_type,
            specification_version,
            specification_asset_class,
            owner_address: owner_address.clone(),
            owner_type,
            creator_address,
            creator_verified,
            authority_address,
            grouping,
            delegate,
            frozen,
            supply,
            supply_mint,
            compressed,
            compressible,
            royalty_target_type,
            royalty_target,
            royalty_amount,
            burnt,
            json_uri,
            name,
            collections,
            token_type,
            created_at,
            tree,
            collection_nft,
            is_agent,
            agent_token,
            asset_signer,
        };

        validate_search_assets_query(&saq, &sort_by)?;

        let sort_by = sort_by.unwrap_or_default();
        let mut options = options.unwrap_or_default();
        options.cdn_prefix = self.cdn_prefix.clone();
        let page_options =
            self.validate_pagination(&limit, &page, &before, &after, &cursor, &Some(&sort_by))?;

        let show_native_balance = options.show_native_balance;
        let owner_pubkey = owner_address
            .clone()
            .map(|x| {
                Pubkey::try_from(x).map_err(|e| {
                    DasApiError::ValidationError(format!("Invalid owner address: {:?}", e))
                })
            })
            .transpose()?;
        let (native_balance, query_result) = tokio::join!(
            async {
                let db = &self.db_lb.next();
                optionally_fetch_native_balance(
                    db,
                    show_native_balance,
                    owner_pubkey,
                    &self.rpc_client,
                )
                .await
            },
            async {
                // Execute query
                if let Some(TokenType::Fungible | TokenType::All) = saq.token_type.clone() {
                    search_tokens(
                        &self.db_lb.next(),
                        saq,
                        sort_by,
                        &page_options,
                        &self.feature_flags,
                        &options.into(),
                    )
                    .await
                    .map_err(|e| DasApiError::from(e))
                } else {
                    search_assets(
                        &self.db_lb.next(),
                        saq,
                        sort_by,
                        &page_options,
                        &self.feature_flags,
                        &options.into(),
                    )
                    .await
                    .map(|al| {
                        if !al.errors.is_empty() {
                            let errs = al.errors.clone();
                            for error in errs {
                                warn!(
                                    "Error building asset response for {}: {}",
                                    error.id, error.error
                                );
                            }
                        }
                        al
                    })
                    .map_err(Into::into)
                }
            }
        );

        let native_balance = native_balance?;
        let mut query_result = query_result?;
        query_result.nativeBalance = native_balance;
        Ok(query_result)
    }

    async fn get_asset_signatures(
        self: &DasApi,
        payload: GetAssetSignatures,
    ) -> Result<TransactionSignatureList, DasApiError> {
        let GetAssetSignatures {
            id,
            limit,
            page,
            before,
            after,
            tree,
            leaf_index,
            cursor,
            sort_direction,
        } = payload;

        if !((id.is_some() && tree.is_none() && leaf_index.is_none())
            || (id.is_none() && tree.is_some() && leaf_index.is_some()))
        {
            return Err(DasApiError::ValidationError(
                "Must provide either 'id' or both 'tree' and 'leafIndex'".to_string(),
            ));
        }
        let id = validate_opt_pubkey(&id)?;
        let tree = validate_opt_pubkey(&tree)?;

        let page_options =
            self.validate_pagination(&limit, &page, &before, &after, &cursor, &None)?;

        get_asset_signatures(
            &self.db_lb.next(),
            id,
            tree,
            leaf_index,
            page_options,
            sort_direction,
        )
        .await
        .map_err(Into::into)
    }

    async fn search_owners(&self, payload: SearchOwners) -> Result<OwnerList, DasApiError> {
        let SearchOwners {
            asset,
            limit,
            page,
            options,
        } = payload;
        let asset_bytes = validate_pubkey(asset.clone())?.to_bytes().to_vec();
        let page_options = self.validate_pagination(&limit, &page, &None, &None, &None, &None)?;
        let options = options.unwrap_or_default();
        search_owners(&self.db_lb.next(), asset_bytes, &page_options, &options)
            .await
            .map_err(Into::into)
    }

    async fn get_token_accounts(
        self: &DasApi,
        payload: GetTokenAccounts,
    ) -> Result<TokenAccountsList, DasApiError> {
        let GetTokenAccounts {
            owner,
            mint,
            limit,
            page,
            before,
            after,
            options,
            cursor,
        } = payload;

        if owner.is_none() && mint.is_none() {
            return Err(DasApiError::ValidationError(
                "Must provide either 'owner' or 'mint'".to_string(),
            ));
        }

        let owner_bytes = validate_opt_pubkey(&owner)?;
        let mint_bytes = validate_opt_pubkey(&mint)?;

        let mut sort_by = TokenSorting::default();
        let page_options =
            self.validate_token_pagination(&limit, &page, &before, &after, &cursor)?;

        if page_options.cursor.is_some() {
            sort_by.sort_by = TokenSortBy::TokenAccount;
        }

        let options = options.unwrap_or_default();

        get_token_accounts(
            &self.db_lb.next(),
            owner_bytes,
            mint_bytes,
            sort_by,
            &page_options,
            &options,
        )
        .await
        .map_err(Into::into)
    }

    async fn get_nft_editions(
        self: &DasApi,
        payload: GetNftEditions,
    ) -> Result<EditionsList, DasApiError> {
        let GetNftEditions { mint, limit, page } = payload;

        if mint.is_none() {
            return Err(DasApiError::ValidationError(
                "Must provide 'mint'".to_string(),
            ));
        }

        let mint_bytes = validate_opt_pubkey(&mint)?;

        let page_options = self.validate_pagination(&limit, &page, &None, &None, &None, &None)?;

        get_nft_editions(&self.db_lb.next(), mint_bytes, &page_options)
            .await
            .map_err(Into::into)
    }
}

fn convert_strings_to_bytes(s: Option<Vec<String>>) -> Result<Option<Vec<Vec<u8>>>, DasApiError> {
    match s {
        Some(v) => {
            let mut ans = Vec::new();
            for x in v {
                if let Some(v) = validate_opt_pubkey(&Some(x))? {
                    ans.push(v);
                }
            }
            Ok(if ans.is_empty() { None } else { Some(ans) })
        }
        None => Ok(None),
    }
}

async fn get_account_balance(rpc_client: &RpcClient, pubkey: Pubkey) -> Result<u64, DasApiError> {
    // Copied config values from get_account_with_commitment. We do not use that function directly
    // it re-emits all the errors as `AccountNotFound` errors, which is misleading.
    let config = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64Zstd),
        commitment: Some(CommitmentConfig {
            commitment: CommitmentLevel::Confirmed,
        }),
        data_slice: None,
        min_context_slot: None,
    };
    let response: Result<Value, Error> = rpc_client
        .send(RpcRequest::GetBalance, json!([pubkey.to_string(), config]))
        .await;

    match response {
        Ok(response_json) => Ok(response_json
            .get("value")
            .ok_or(DasApiError::InternalError(format!(
                "Error getting account info for {pubkey}. Value is missing"
            )))?
            .as_u64()
            .unwrap_or(0)),
        Err(e) => {
            let e = Err(DasApiError::InternalError(format!(
                "Error getting account info for {pubkey}. {e}"
            )));
            e
        }
    }
}

async fn get_sol_price_from_db(db: &DatabaseConnection) -> Result<f64, DasApiError> {
    // Get price from the price table
    let mint_bytes = bs58::decode(FAKE_SOL_PUBKEY)
        .into_vec()
        .map_err(|_| DasApiError::InternalError("Invalid 'id' format".to_string()))?;

    let price = price::Entity::find()
        .filter(price::Column::Mint.eq(mint_bytes))
        .one(db)
        .await?
        .ok_or(DasApiError::InternalError(
            "SOL price not found".to_string(),
        ))?;

    price
        .price
        .ok_or(DasApiError::InternalError(
            "SOL price not found".to_string(),
        ))
        .map(|price| price as f64)
}

async fn get_sol_price_from_db_with_retry(db: &DatabaseConnection) -> Result<f64, DasApiError> {
    let retries = 3;
    for _ in 0..retries - 1 {
        if let Ok(price) = get_sol_price_from_db(db).await {
            return Ok(price);
        }
    }
    get_sol_price_from_db(db).await
}

// Timeout caps RpcClient's hardcoded ~7.5s of internal 429 retry sleeps.
const ACCOUNT_BALANCE_ATTEMPTS: usize = 2;
const ACCOUNT_BALANCE_ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(800);

async fn get_account_balance_with_retry(
    rpc_client: &RpcClient,
    pubkey: Pubkey,
) -> Result<u64, DasApiError> {
    for _ in 0..ACCOUNT_BALANCE_ATTEMPTS - 1 {
        if let Ok(Ok(balance)) = tokio::time::timeout(
            ACCOUNT_BALANCE_ATTEMPT_TIMEOUT,
            get_account_balance(rpc_client, pubkey),
        )
        .await
        {
            return Ok(balance);
        }
    }
    tokio::time::timeout(
        ACCOUNT_BALANCE_ATTEMPT_TIMEOUT,
        get_account_balance(rpc_client, pubkey),
    )
    .await
    .map_err(|_| {
        DasApiError::InternalError("Timed out fetching account balance".to_string())
    })?
}

async fn optionally_fetch_native_balance(
    db: &DatabaseConnection,
    show_native_balance: bool,
    owner_address: Option<Pubkey>,
    rpc_client: &RpcClient,
) -> Result<Option<NativeBalance>, DasApiError> {
    if show_native_balance {
        if let Some(owner_address) = owner_address {
            let owner_address = Pubkey::try_from(owner_address).map_err(|e| {
                DasApiError::ValidationError(format!("Invalid owner address: {:?}", e))
            })?;
            match fetch_native_balance(db, rpc_client, owner_address).await {
                Ok(balance) => Ok(Some(balance)),
                Err(e) => {
                    log::error!("Error fetching native balance: {:?}", e);
                    Ok(None)
                }
            }
        } else {
            Err(DasApiError::ValidationError(
                "Must provide `owner_address` when using `show_native_balance` field".to_string(),
            ))
        }
    } else {
        Ok(None)
    }
}

async fn fetch_native_balance(
    db: &DatabaseConnection,
    rpc_client: &RpcClient,
    owner: Pubkey,
) -> Result<NativeBalance, DasApiError> {
    let (native_balance, price_per_sol) = tokio::join!(
        get_account_balance_with_retry(rpc_client, owner),
        get_sol_price_from_db_with_retry(db)
    );
    let native_balance = native_balance?;
    let price_per_sol = price_per_sol?;
    Ok(NativeBalance {
        lamports: native_balance,
        price_per_sol,
        total_price: price_per_sol * (native_balance as f64 / 10u64.pow(9) as f64),
    })
}

#[cfg(test)]
mod account_balance_retry_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    fn spawn_mock_rpc(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
        let addr = listener.local_addr().expect("mock rpc addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                    let _ = stream.read(&mut buf);
                    let _ = stream.write_all(response.as_bytes());
                });
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn rate_limited_upstream_fails_within_attempt_timeouts() {
        let url = spawn_mock_rpc(
            "HTTP/1.1 429 Too Many Requests\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
        let rpc_client = RpcClient::new(url);
        let start = Instant::now();
        let result = get_account_balance_with_retry(&rpc_client, Pubkey::new_unique()).await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "429 upstream must not yield a balance");
        // Without the per-attempt cap the client sleeps ~7.5s in 429 backoff; the
        // bound proves the cap engaged (2 attempts x 800ms, plus scheduling slack).
        assert!(
            elapsed < Duration::from_millis(3000),
            "expected fail-fast, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn healthy_upstream_returns_balance() {
        let body = r#"{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":42},"id":1}"#;
        let response: &'static str = Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .into_boxed_str(),
        );
        let url = spawn_mock_rpc(response);
        let rpc_client = RpcClient::new(url);
        let balance = get_account_balance_with_retry(&rpc_client, Pubkey::new_unique())
            .await
            .expect("healthy upstream returns balance");
        assert_eq!(balance, 42);
    }
}
