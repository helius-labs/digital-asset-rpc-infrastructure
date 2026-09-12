use crate::dao::extensions::instruction::PascalCase;
use crate::dao::scopes::asset_metadata::get_asset_and_metadata;
use crate::dao::{extensions, owners};
use crate::dapi::common::filter_non_null_fields;
use crate::rpc::filter::AssetSortDirection;
use crate::rpc::{Authority, Group, GroupDefinition, Owner, Scope, TokenAccount};
use crate::{
    dao::{
        asset::{self, Entity},
        asset_creators, asset_data_v2, cl_audits_v2, offchain_metadata, price,
        sea_orm_active_enums::{
            Instruction, OwnerType, SpecificationAssetClass, SpecificationVersions,
        },
        tokens, AssetMetadata, Cursor, FullAsset, Pagination, PriceInfo,
        TokenAccount as DaoTokenAccount, TokenInfo,
    },
    dapi::common::safe_select,
    rpc::{options::Options, Asset, CollectionMetadata},
};
use indexmap::IndexMap;
use log::error;
use num_traits::ToPrimitive;
use schemars::JsonSchema;
use sea_orm::{
    entity::*, query::*, sea_query::SimpleExpr, ConnectionTrait, DbBackend, DbErr, FromQueryResult,
    Order,
};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};
use tokio::try_join;

use super::nft_editions::{get_related_edition, get_related_editions};
use super::owner::{OwnerAssetInfo, OwnerFungibleAssetInfo, OwnerResult, OwnerTokenInfo};

const SPL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_ID: &str =
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all(serialize = "camelCase", deserialize = "camelCase"))]
pub enum TokenType {
    Fungible,
    NonFungible,
    CompressedNft,
    RegularNft,
    All,
}

#[derive(Debug)]
pub struct Clause {
    order_clause: String,
    outer_order_clause: String,
    offset_clause: String,
    keyset_clause: String,
    balance_clause: String,
    collection_clause: String,
    asset_clause: String,
}

pub fn find_associated_token_address(
    owner: Pubkey,
    mint: Pubkey,
    program_id: Option<Pubkey>,
) -> Result<Pubkey, DbErr> {
    let associated_token_program_id = Pubkey::from_str(SPL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_ID)
        .map_err(|_| DbErr::Type(SPL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_ID.to_owned()))?;

    let token_program_id = program_id.ok_or(DbErr::Type("invalid program id".to_owned()))?;

    Ok(Pubkey::find_program_address(
        &[owner.as_ref(), token_program_id.as_ref(), mint.as_ref()],
        &associated_token_program_id,
    )
    .0)
}

pub fn paginate<'db, T, C>(
    pagination: &Pagination,
    limit: u64,
    stmt: T,
    sort_direction: Order,
    column: C,
) -> T
where
    T: QueryFilter + QuerySelect,
    C: ColumnTrait,
{
    let mut stmt = stmt;
    match pagination {
        Pagination::Keyset { before, after } => {
            if let Some(b) = before {
                stmt = stmt.filter(column.lt(b.clone()));
            }
            if let Some(a) = after {
                stmt = stmt.filter(column.gt(a.clone()));
            }
        }
        Pagination::Page { page } => {
            if *page > 0 {
                stmt = stmt.offset((page - 1) * limit)
            }
        }
        Pagination::Cursor(cursor) => {
            if *cursor != Cursor::default() {
                if sort_direction == sea_orm::Order::Asc {
                    stmt = stmt.filter(column.gt(cursor.id.clone()));
                } else {
                    stmt = stmt.filter(column.lt(cursor.id.clone()));
                }
            }
        }
    }
    stmt.limit(limit)
}

pub fn get_clauses(
    sort_by: Option<impl ColumnTrait>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    options: &Options,
    collection_id: Option<String>,
    search_includes_compressed_nfts: bool,
    asset_id: Option<Vec<u8>>,
) -> Clause {
    let (order_clause, outer_order_clause, offset_clause, keyset_clause) =
        raw_owner_paginate(sort_by, sort_direction, pagination, limit);
    let balance_clause = get_optional_clauses(options, search_includes_compressed_nfts);
    let mut collection_clause = String::new();
    if let Some(collection_id) = collection_id {
        collection_clause = format!(
            "AND collections_info ->> 'collection_id' = '{}'",
            collection_id
        );
    }
    let asset_clause = match asset_id {
        Some(asset_id) => format!("WHERE ad.mint = E'\\\\x{}'", hex::encode(asset_id)),
        None => "".to_string(),
    };
    Clause {
        order_clause,
        outer_order_clause,
        offset_clause,
        keyset_clause,
        balance_clause,
        collection_clause,
        asset_clause,
    }
}

pub fn raw_owner_paginate(
    sort_by: Option<impl ColumnTrait>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
) -> (String, String, String, String) {
    let mut offset_clause = String::new();
    let mut keyset_clause = String::new();
    let mut order_clause = format!("ORDER BY mint desc");
    let mut outer_order_clause = format!("ORDER BY ad.mint desc");

    if let Some(sort_column) = sort_by {
        let direction = match sort_direction {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
            Order::Field(_) => todo!(),
        };
        let sort_column = sort_column.to_string();

        order_clause = format!("ORDER BY {} {}", sort_column.to_string(), direction);
        outer_order_clause = format!("ORDER BY ad.{} {}", sort_column.to_string(), direction);
    }

    match pagination {
        Pagination::Keyset {
            before: Some(before),
            ..
        } => {
            keyset_clause = format!(
                "AND {} < '\\x{}'",
                sort_by.unwrap().to_string(),
                hex::encode(before)
            );
        }
        Pagination::Keyset {
            after: Some(after), ..
        } => {
            keyset_clause = format!(
                "AND {} > '\\x{}'",
                sort_by.unwrap().to_string(),
                hex::encode(after)
            );
        }
        Pagination::Page { page } => {
            let offset = (page - 1) * limit;
            offset_clause = format!("OFFSET {}", offset);
        }
        Pagination::Cursor(Cursor {
            id: Some(cursor_id),
        }) => {
            let comparison_op = match sort_direction {
                Order::Asc => ">",
                Order::Desc => "<",
                Order::Field(_) => todo!(),
            };
            keyset_clause = format!(
                "AND {} {} '\\x{}'",
                sort_by.unwrap().to_string(),
                comparison_op,
                hex::encode(cursor_id)
            );
        }
        _ => {}
    }

    (
        order_clause,
        outer_order_clause,
        offset_clause,
        keyset_clause,
    )
}

pub fn get_optional_clauses(options: &Options, search_includes_compressed_nfts: bool) -> String {
    match options.show_zero_balance {
        true => "".to_string(),
        // We don't populate balances for compressed NFTs.
        false if search_includes_compressed_nfts => {
            "WHERE balance > 0 OR asset_compressed = true".to_string()
        }
        false => "WHERE balance > 0".to_string(),
    }
}

pub async fn get_by_owner(
    conn: &impl ConnectionTrait,
    owner: Vec<u8>,
    sort_by: Option<owners::Column>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
    options: &Options,
    token_type: Option<TokenType>,
    collection_id: Option<String>,
    asset_id: Option<Vec<u8>>,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    if token_type == Some(TokenType::Fungible) {
        get_related_fungible_assets_by_owner(
            conn,
            owner,
            sort_by,
            sort_direction,
            pagination,
            limit,
            options,
            collection_id,
            asset_id,
        )
        .await
    } else {
        get_related_assets_by_owner(
            conn,
            owner,
            sort_by,
            sort_direction,
            pagination,
            limit,
            enable_grand_total_query,
            options,
            collection_id,
            asset_id,
        )
        .await
    }
}

pub async fn get_by_creator(
    conn: &impl ConnectionTrait,
    creator: Vec<u8>,
    only_verified: bool,
    sort_by: Option<asset::Column>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
    options: &Options,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let mut condition: Condition;
    condition = Condition::all()
        .add(asset_creators::Column::Creator.eq(creator))
        .add(asset::Column::Supply.ne(0));
    if only_verified {
        condition = condition.add(asset_creators::Column::Verified.eq(true));
    }
    get_by_related_condition(
        conn,
        condition,
        extensions::asset::Relation::AssetCreators,
        sort_by,
        sort_direction,
        pagination,
        limit,
        enable_grand_total_query,
        options,
    )
    .await
}

pub async fn get_by_grouping(
    conn: &impl ConnectionTrait,
    group_key: String,
    group_value: String,
    sort_by: Option<asset::Column>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
    options: &Options,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let mut condition = Condition::all().add(asset::Column::Supply.ne(0));

    // group_value is a validated pubkey (see validate_pubkey). Querying the
    // `groups` sub-array (not the whole doc) lets the GIN index on
    // (collections_info -> 'groups') serve the containment.
    if group_key == "group" {
        condition = condition.add(SimpleExpr::Custom(
            format!(
                "collections_info -> 'groups' @> '[\"{}\"]'::jsonb",
                group_value
            )
            .into(),
        ));
        return get_assets_by_condition(
            conn,
            condition,
            vec![],
            sort_by,
            sort_direction,
            pagination,
            limit,
            enable_grand_total_query,
            options,
        )
        .await;
    }

    let collection_expr = SimpleExpr::Custom(
        format!("collections_info ->> 'collection_id' = '{}'", group_value).into(),
    );
    condition = condition.add(collection_expr);

    if !options.show_unverified_collections {
        let verified_expr = SimpleExpr::Custom("collections_info @> '{\"verified\": true}'".into());
        let null_string_expr =
            SimpleExpr::Custom("(collections_info ->> 'verified')::text IS NULL".into());
        let null_expr = SimpleExpr::Custom("collections_info -> 'verified' IS NULL".into());

        condition = condition.add(verified_expr.or(null_string_expr).or(null_expr));
    }

    get_assets_by_condition(
        conn,
        condition,
        vec![],
        sort_by,
        sort_direction,
        pagination,
        limit,
        enable_grand_total_query,
        options,
    )
    .await
}

pub async fn get_assets_by_owner(
    conn: &impl ConnectionTrait,
    owner: Vec<u8>,
    sort_by: Option<asset::Column>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
    options: &Options,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let cond = Condition::all()
        .add(asset::Column::Owner.eq(owner))
        .add(asset::Column::Supply.gt(0));
    get_assets_by_condition(
        conn,
        cond,
        vec![],
        sort_by,
        sort_direction,
        pagination,
        limit,
        enable_grand_total_query,
        options,
    )
    .await
}

pub async fn get_owners(
    conn: &impl ConnectionTrait,
    asset: Vec<u8>,
    pagination: &Pagination,
    limit: u64,
    options: &Options,
) -> Result<Vec<Owner>, DbErr> {
    let mut query = owners::Entity::find()
        .select_only()
        .distinct()
        .column(owners::Column::Owner)
        .column(owners::Column::TokenProgram)
        .filter(owners::Column::Mint.eq(asset.clone()));

    if !options.show_zero_balance {
        let cond = Condition::any()
            .add(owners::Column::TokenAmount.gt(0))
            .add(owners::Column::TokenAmount.is_null());
        query = query.filter(cond);
    }

    // for getting consistent results in pagination
    query = query.order_by_asc(owners::Column::Owner);

    query = paginate(
        pagination,
        limit,
        query,
        sea_orm::Order::Asc, // not used
        owners::Column::Id,  // not used
    );

    let stmt = query.build(DbBackend::Postgres);
    let owner_results: Vec<OwnerResult> = conn.query_all(stmt).await.map(|qr| {
        qr.iter()
            .map(|q| OwnerResult::from_query_result(q, "").unwrap())
            .collect()
    })?;

    let owners = owner_results
        .into_iter()
        .map(|o| Owner {
            id: bs58::encode(&o.owner).into_string(),
            associated_token_address: match o.token_program {
                None => None,
                Some(token_program) => {
                    let ata = find_associated_token_address(
                        Pubkey::try_from(o.owner).unwrap(),
                        Pubkey::try_from(asset.clone()).unwrap(),
                        Some(Pubkey::try_from(token_program).unwrap()),
                    );
                    match ata {
                        Ok(ata) => Some(bs58::encode(ata).into_string()),
                        Err(_) => None,
                    }
                }
            },
        })
        .collect::<Vec<_>>();
    Ok(owners)
}

pub async fn get_token_accounts(
    conn: &impl ConnectionTrait,
    owner: Option<Vec<u8>>,
    mint: Option<Vec<u8>>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    options: &Options,
) -> Result<Vec<TokenAccount>, DbErr> {
    let mut query = owners::Entity::find()
        .filter(owners::Column::TokenAccount.is_not_null())
        .filter(owners::Column::Closed.eq(false));

    if !options.show_zero_balance {
        query = query.filter(owners::Column::TokenAmount.ne(0));
    }
    if let Some(owner) = owner {
        query = query.filter(owners::Column::Owner.eq(owner));
    }
    if let Some(mint) = mint {
        query = query.filter(owners::Column::Mint.eq(mint));
    }

    query = query.order_by_asc(owners::Column::TokenAccount);
    query = paginate(
        pagination,
        limit,
        query,
        sort_direction,
        owners::Column::TokenAccount,
    );

    let token_accounts = query.all(conn).await?;

    let token_accounts: Vec<TokenAccount> = token_accounts
        .into_iter()
        .filter_map(|model| {
            model.token_account.map(|ta| TokenAccount {
                address: bs58::encode(ta).into_string(),
                mint: model.mint.map(|mint| bs58::encode(mint).into_string()),
                owner: model.owner.map(|owner| bs58::encode(owner).into_string()),
                amount: model
                    .token_amount_u64
                    .map(|amount| amount.to_u64().unwrap_or(0)),
                delegate: model
                    .delegate
                    .map(|delegate| bs58::encode(delegate).into_string()),
                delegated_amount: model.delegated_amount.map(|amount| amount as u64),
                frozen: model.frozen,
                token_extensions: filter_non_null_fields(model.token_extensions.as_ref()),
            })
        })
        .collect();

    Ok(token_accounts)
}

pub async fn get_assets(
    conn: &impl ConnectionTrait,
    asset_ids: Vec<Vec<u8>>,
    pagination: &Pagination,
    limit: u64,
    options: &Options,
) -> Result<Vec<FullAsset>, DbErr> {
    let mut assets = Vec::new();
    let get_assets_future =
        get_asset_nft_batch(conn, asset_ids.clone(), pagination, limit, options);
    let get_fungible_assets_future =
        get_asset_all_batch(conn, asset_ids.clone(), pagination, limit, options);

    let (get_assets_result, get_fungible_assets_result) =
        tokio::join!(get_assets_future, get_fungible_assets_future);

    let get_assets = get_assets_result?;
    let get_fungible_assets = get_fungible_assets_result?;

    let mut assets_index = std::collections::HashMap::new();
    for asset in get_assets {
        let key = asset.asset.id.to_vec();
        assets_index.insert(key, asset);
    }

    for asset_id in asset_ids {
        if let Some(mut asset) = assets_index.get(&asset_id).cloned() {
            if let Some(fungible_asset) =
                get_fungible_assets.iter().find(|a| a.asset.id == asset_id)
            {
                asset.token_info = fungible_asset.token_info.clone();
                asset.asset.mint_extensions = fungible_asset.asset.mint_extensions.clone();
            }
            assets.push(asset);
        } else if let Some(asset) = get_fungible_assets.iter().find(|a| a.asset.id == asset_id) {
            assets.push(asset.clone());
        }
    }
    Ok(assets)
}

pub async fn get_asset_all_batch(
    conn: &impl ConnectionTrait,
    asset_ids: Vec<Vec<u8>>,
    _pagination: &Pagination,
    _limit: u64,
    _options: &Options,
) -> Result<Vec<FullAsset>, DbErr> {
    let conds = Condition::all().add(tokens::Column::Mint.is_in(asset_ids));

    let stmt = tokens::Entity::find()
        .find_also_related(price::Entity)
        .filter(conds)
        .all(conn)
        .await?;

    let mut full_assets = Vec::new();
    for (mint, price) in stmt {
        let price_info = price.clone().map(|price| PriceInfo {
            price_per_token: price.price,
            currency: Some("USDC".to_string()),
            total_price: None,
        });

        let token_info = TokenInfo {
            symbol: price.map(|price| price.symbol).unwrap_or_default(),
            price_info,
            supply: Some(mint.supply as u64),
            decimals: Some(mint.decimals),
            balance: None,
            associated_token_address: None,
            token_program: Some(bs58::encode(mint.token_program.clone()).into_string()),
            mint_authority: mint
                .mint_authority
                .as_ref()
                .map(|auth| bs58::encode(auth).into_string()),
            freeze_authority: mint
                .freeze_authority
                .as_ref()
                .map(|auth| bs58::encode(auth).into_string()),
            token_accounts: None,
        };

        full_assets.push(FullAsset {
            asset: asset::Model::from(mint),
            data: AssetMetadata::default(),
            authorities: vec![],
            creators: vec![],
            groups: vec![],
            token_info: Some(token_info),
            editions: None,
            group_definition: None,
        });
    }

    Ok(full_assets)
}

pub async fn get_asset_nft_batch(
    conn: &impl ConnectionTrait,
    asset_ids: Vec<Vec<u8>>,
    pagination: &Pagination,
    limit: u64,
    options: &Options,
) -> Result<Vec<FullAsset>, DbErr> {
    let mut conds = Condition::all().add(asset::Column::Id.is_in(asset_ids));
    // Dirty fix for Tensor.
    // Collections info is the last thing to update during cNFT mint (mint_v1.rs).
    // We check that it is not null. If it's not null, then the asset is fully indexed.
    if options.require_full_index {
        conds = conds.add(asset::Column::CollectionsInfo.is_not_null());
    }
    let (assets, _grand_total) = get_assets_by_condition(
        conn,
        conds,
        vec![],
        // Default values provided. The args below are not used for batch requests
        None,
        Order::Asc,
        pagination,
        limit,
        false,
        options,
    )
    .await?;
    Ok(assets)
}

pub async fn get_by_authority(
    conn: &impl ConnectionTrait,
    authority: Vec<u8>,
    sort_by: Option<asset::Column>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
    options: &Options,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    // Use the indexed authority_address column instead of JSONB authorities_info
    // This allows PostgreSQL to use idx_asset_authority_address_id index
    let cond = Condition::all()
        .add(asset::Column::AuthorityAddress.eq(authority))
        .add(asset::Column::Supply.ne(0));

    get_assets_by_condition(
        conn,
        cond,
        vec![],
        sort_by,
        sort_direction,
        pagination,
        limit,
        enable_grand_total_query,
        options,
    )
    .await
}

fn validate_no_duplicate_mints(mints: Vec<Vec<u8>>) -> Result<(), DbErr> {
    let seen: HashSet<Vec<u8>> = mints.clone().into_iter().collect();
    if seen.len() != mints.len() {
        return Err(DbErr::Custom("Duplicate mint found".to_string()));
    }
    Ok(())
}

async fn get_related_fungible_assets_by_owner(
    conn: &impl ConnectionTrait,
    owner: Vec<u8>,
    sort_by: Option<owners::Column>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    options: &Options,
    collection_id: Option<String>,
    asset_id: Option<Vec<u8>>,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let search_includes_compressed_nfts = false;
    let clauses = get_clauses(
        sort_by,
        sort_direction,
        pagination,
        limit,
        options,
        collection_id,
        search_includes_compressed_nfts,
        asset_id,
    );

    let raw_sql = format!(
        r#"
        WITH AggregatedData AS (
            SELECT
                sub.owner,
                sub.mint,
                sub.balance,
                sub.token_accounts,
                sub.asset_data,
                sub.authority_address as authority_address,
                sub.authority_scopes as authority_scopes,
                sub.authority_slot_updated as authority_slot_updated,
                sub.authority_seq as authority_seq,
                sub.authorities_info,
                sub.specification_asset_class,
                sub.creators_info,
                sub.collections_info,
                sub.slot_updated,
                sub.created_at
            FROM (
                SELECT
                    o.owner,
                    o.mint,
                    SUM(COALESCE(o.token_amount_u64, 0)) AS balance,
                    jsonb_agg(
                        jsonb_build_object(
                            'token_account', o.token_account,
                            'balance', o.token_amount_u64
                        )
                    ) AS token_accounts,
                    a.asset_data as asset_data,
                    a.authority_address as authority_address,
                    a.authority_scopes as authority_scopes,
                    a.authority_slot_updated as authority_slot_updated,
                    a.authority_seq as authority_seq,
                    a.authorities_info as authorities_info,
                    a.specification_asset_class as specification_asset_class,
                    a.creators_info as creators_info,
                    a.collections_info as collections_info,
                    a.slot_updated,
                    a.created_at
                FROM
                    owners o
                LEFT JOIN
                    asset a ON o.mint = a.id
                WHERE
                    o.owner = $1  AND
                    (
                        a.owner_type != 'single' OR
                        a.owner_type IS NULL OR
                        (a.specification_asset_class = 'unknown' AND a.supply > 1)
                    )
                    {}
                    {}
                GROUP BY
                    o.owner, o.mint, a.asset_data, a.authorities_info, a.authority_address, a.authority_scopes,
                    a.authority_slot_updated, a.authority_seq, a.specification_asset_class,
                    a.creators_info, a.collections_info, a.slot_updated, a.created_at
                {}
            ) sub
            {}
            {}
            LIMIT $2
        )
        SELECT
            ad.balance as balance,
            ad.asset_data as asset_data,
            ad.token_accounts as token_accounts,
            ad.authority_address as asset_authority_address,
            ad.authority_scopes as asset_authority_scopes,
            ad.authority_slot_updated as asset_authority_slot_updated,
            ad.authority_seq as asset_authority_seq,
            ad.authorities_info as asset_authorities_info,
            ad.specification_asset_class::text as specification_asset_class,
            ad.creators_info as asset_creators_info,
            ad.collections_info as asset_collections_info,
            ad.owner,
            ad.mint,
            t.token_program as mint_token_program,
            t.supply as mint_supply,
            t.decimals as mint_decimals,
            t.mint_authority as mint_authority,
            t.freeze_authority as mint_freeze_authority,
            t.close_authority as mint_close_authority,
            t.extensions as mint_extensions,
            p.price,
            p.symbol as price_symbol,
            adv.metadata_url,
            adv.chain_mutability::text as chain_mutability,
            adv.chain_data,
            adv.raw_name,
            adv.raw_symbol,
            om.metadata as offchain_metadata
        FROM
            AggregatedData ad
        LEFT JOIN
            tokens t ON ad.mint = t.mint
        LEFT JOIN
            price p ON ad.mint = p.mint
        LEFT JOIN
            asset_data_v2 adv ON ad.asset_data = adv.id
        LEFT JOIN
            offchain_metadata om ON adv.metadata_url = om.metadata_url
        {}
        {};
    "#,
        clauses.collection_clause,
        clauses.keyset_clause,
        clauses.order_clause,
        clauses.balance_clause,
        clauses.offset_clause,
        clauses.asset_clause,
        // HACK: We include the order clause again to actually order the final selection set.
        //       There is probably a cleaner way to do this, but wanted to do this with minimal changes.
        clauses.outer_order_clause
    );

    let owner_assets: Vec<OwnerFungibleAssetInfo> =
        OwnerFungibleAssetInfo::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            &raw_sql,
            vec![owner.into(), limit.into()],
        ))
        .all(conn)
        .await?;

    validate_no_duplicate_mints(owner_assets.iter().map(|x| x.mint.clone()).collect())?;

    let full_assets = get_full_assets_for_owner(conn, owner_assets, options).await?;
    Ok((full_assets, None))
}

async fn get_related_assets_by_owner(
    conn: &impl ConnectionTrait,
    owner: Vec<u8>,
    sort_by: Option<owners::Column>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    _enable_grand_total_query: bool,
    options: &Options,
    collection_id: Option<String>,
    asset_id: Option<Vec<u8>>,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let search_includes_compressed_nfts = true;
    let clauses = get_clauses(
        sort_by,
        sort_direction,
        pagination,
        limit,
        options,
        collection_id,
        search_includes_compressed_nfts,
        asset_id,
    );

    let raw_sql = format!(
        r#"
        WITH AggregatedData AS (
            SELECT
                sub.owner,
                sub.mint,
                sub.balance,
                sub.token_accounts,
                sub.slot_updated,
                sub.created_at,
                sub.asset_frozen,
                sub.asset_delegate,
                sub.specification_version,
                sub.specification_asset_class,
                sub.asset_supply,
                sub.asset_compressed,
                sub.asset_compressible,
                sub.asset_seq,
                sub.asset_tree_id,
                sub.asset_leaf,
                sub.asset_nonce,
                sub.asset_owner_type,
                sub.asset_royalty_target_type,
                sub.asset_royalty_target,
                sub.asset_royalty_amount,
                sub.mpl_core_plugins_json_version,
                sub.mpl_core_plugins,
                sub.mpl_core_external_plugins,
                sub.mpl_core_unknown_external_plugins,
                sub.asset_data,
                sub.asset_burnt,
                sub.asset_data_hash,
                sub.asset_creator_hash,
                sub.asset_leaf_seq,
                sub.asset_creators_info,
                sub.asset_collections_info,
                sub.asset_authority_address,
                sub.asset_authority_scopes,
                sub.asset_authority_seq,
                sub.asset_authority_slot_updated,
                sub.asset_authorities_info,
                sub.asset_mint_extensions,
                sub.edition_address,
                sub.metadata_account_id
            FROM (
                SELECT
                    o.owner,
                    o.mint,
                    SUM(COALESCE(o.token_amount_u64, 0)) AS balance,
                    jsonb_agg(
                        jsonb_build_object(
                            'token_account', o.token_account,
                            'balance', o.token_amount_u64
                        )
                    ) AS token_accounts,
                    a.slot_updated,
                    a.created_at,
                    a.compressed AS asset_compressed,
                    a.specification_asset_class,
                    a.owner_type AS asset_owner_type,
                    a.supply AS asset_supply,
                    a.owner AS asset_owner,
                    a.frozen AS asset_frozen,
                    a.delegate AS asset_delegate,
                    a.specification_version,
                    a.compressible AS asset_compressible,
                    a.seq AS asset_seq,
                    a.tree_id AS asset_tree_id,
                    a.leaf AS asset_leaf,
                    a.nonce AS asset_nonce,
                    a.royalty_target_type AS asset_royalty_target_type,
                    a.royalty_target AS asset_royalty_target,
                    a.royalty_amount AS asset_royalty_amount,
                    a.mpl_core_plugins_json_version,
                    a.mpl_core_plugins,
                    a.mpl_core_external_plugins,
                    a.mpl_core_unknown_external_plugins,
                    a.asset_data,
                    a.burnt AS asset_burnt,
                    a.data_hash AS asset_data_hash,
                    a.creator_hash AS asset_creator_hash,
                    a.leaf_seq AS asset_leaf_seq,
                    a.creators_info AS asset_creators_info,
                    a.collections_info AS asset_collections_info,
                    a.authority_address AS asset_authority_address,
                    a.authority_scopes AS asset_authority_scopes,
                    a.authority_seq AS asset_authority_seq,
                    a.authority_slot_updated AS asset_authority_slot_updated,
                    a.authorities_info AS asset_authorities_info,
                    a.mint_extensions AS asset_mint_extensions,
                    a.edition_address,
                    a.metadata_account_id
                FROM
                    owners o
                LEFT JOIN
                    asset a ON o.mint = a.id
                WHERE
                    o.owner = $1 AND
                    (
                        a.compressed IS NULL OR a.compressed = false OR
                        (a.compressed = true AND a.owner = o.owner)
                    ) AND
                    (
                        a.specification_asset_class IS NULL OR a.specification_asset_class != 'PROGRAMMABLE_NFT' OR
                        (a.specification_asset_class = 'PROGRAMMABLE_NFT' AND a.owner = o.owner)
                    ) AND
                    (
                        a.specification_asset_class IS NULL OR a.specification_asset_class != 'NFT' OR
                        (a.specification_asset_class = 'NFT' AND a.owner = o.owner)
                    ) AND
                    (
                        a.specification_asset_class IS NULL OR a.specification_asset_class != 'unknown' OR a.supply > 1 OR a.owner_type != 'single' OR
                        (a.specification_asset_class = 'unknown' AND a.owner = o.owner AND a.owner_type = 'single' AND a.supply = 1)
                    )
                    {}
                    {}
                GROUP BY
                    o.owner, o.mint, a.compressed, a.owner, a.specification_asset_class, a.owner_type,
                    a.supply, a.created_at, a.slot_updated, a.frozen, a.delegate, a.specification_version,
                    a.compressible, a.seq, a.tree_id, a.leaf, a.nonce, a.royalty_target_type,
                    a.royalty_target, a.royalty_amount, a.mpl_core_plugins_json_version, a.mpl_core_plugins,
                    a.mpl_core_external_plugins, a.mpl_core_unknown_external_plugins, a.asset_data,
                    a.burnt, a.data_hash, a.creator_hash, a.leaf_seq, a.creators_info, a.collections_info,
                    a.authority_address, a.authority_scopes, a.authority_seq, a.authority_slot_updated,
                    a.authorities_info, a.mint_extensions, a.edition_address, a.metadata_account_id
                {}
            ) sub
            {}
            {}
            LIMIT $2
        )
        SELECT
            ad.balance as balance,
            ad.token_accounts as token_accounts,
            ad.owner,
            ad.mint,
            ad.asset_frozen,
            ad.asset_delegate,
            ad.specification_version::text as specification_version,
            ad.specification_asset_class::text as specification_asset_class,
            ad.asset_supply,
            ad.asset_compressed,
            ad.asset_compressible,
            ad.asset_seq,
            ad.asset_tree_id,
            ad.asset_leaf,
            ad.asset_nonce,
            ad.asset_owner_type::text as asset_owner_type,
            ad.asset_royalty_target_type::text as asset_royalty_target_type,
            ad.asset_royalty_target,
            ad.asset_royalty_amount,
            ad.mpl_core_plugins_json_version,
            ad.mpl_core_plugins,
            ad.mpl_core_external_plugins,
            ad.mpl_core_unknown_external_plugins,
            ad.asset_data,
            ad.asset_burnt,
            ad.asset_data_hash,
            ad.asset_creator_hash,
            ad.asset_leaf_seq,
            ad.asset_creators_info,
            ad.asset_collections_info,
            ad.asset_authority_address,
            ad.asset_authority_scopes,
            ad.asset_authority_seq,
            ad.asset_authority_slot_updated,
            ad.asset_authorities_info,
            ad.asset_mint_extensions,
            ad.edition_address,
            ad.created_at,
            t.supply as mint_supply,
            t.decimals as mint_decimals,
            t.mint_authority as mint_authority,
            t.freeze_authority as mint_freeze_authority,
            t.close_authority as mint_close_authority,
            t.extensions as mint_extensions,
            t.token_program as mint_token_program,
            p.price,
            p.symbol as price_symbol,
            ad.metadata_account_id,
            adv.metadata_url,
            adv.chain_mutability::text as chain_mutability,
            adv.chain_data,
            adv.raw_name,
            adv.raw_symbol,
            om.metadata as offchain_metadata
        FROM
            AggregatedData ad
        LEFT JOIN
            tokens t ON ad.mint = t.mint
        LEFT JOIN
            price p ON ad.mint = p.mint
        LEFT JOIN
            asset_data_v2 adv ON ad.asset_data = adv.id
        LEFT JOIN
            offchain_metadata om ON adv.metadata_url = om.metadata_url
        {}
        {}
    "#,
        clauses.collection_clause,
        clauses.keyset_clause,
        clauses.order_clause,
        clauses.balance_clause,
        clauses.offset_clause,
        clauses.asset_clause,
        // HACK: We include the order clause again to actually order the final selection set.
        //       There is probably a cleaner way to do this, but wanted to do this with minimal changes.
        clauses.outer_order_clause,
    );

    let owner_assets: Vec<OwnerAssetInfo> =
        OwnerAssetInfo::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            &raw_sql,
            vec![owner.into(), limit.into()],
        ))
        .all(conn)
        .await?;

    validate_no_duplicate_mints(owner_assets.iter().map(|x| x.mint.clone()).collect())?;

    let full_assets = get_full_assets_for_owner(conn, owner_assets, options).await?;

    Ok((full_assets, None))
}

async fn get_by_related_condition<E>(
    conn: &impl ConnectionTrait,
    condition: Condition,
    relation: E,
    sort_by: Option<asset::Column>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
    options: &Options,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr>
where
    E: RelationTrait,
{
    let mut stmt = asset::Entity::find()
        .filter(condition)
        .join(JoinType::LeftJoin, relation.def());

    if let Some(col) = sort_by {
        stmt = stmt
            .order_by(col, sort_direction.clone())
            .order_by(asset::Column::Id, sort_direction.clone());
    }

    let (assets, grand_total) = get_full_response(
        conn,
        stmt,
        pagination,
        limit,
        sort_by,
        sort_direction,
        enable_grand_total_query,
        options,
    )
    .await?;
    Ok((assets, grand_total))
}

fn get_asset_authorities(asset: &asset::Model) -> Vec<Authority> {
    asset
        .authorities_info
        .as_ref()
        .map_or(Vec::new(), |authorities_info| {
            let auth: Vec<u8> = authorities_info["authority"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|val| val.as_u64().unwrap_or(0) as u8)
                .collect();
            let scopes: Vec<Scope> = authorities_info["scopes"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|s| Scope::from(s.as_str().unwrap_or_default().to_string()))
                .collect();
            vec![Authority {
                asset_id: asset.id.clone(),
                address: bs58::encode(auth).into_string(),
                scopes: if scopes.is_empty() {
                    vec![Scope::Full]
                } else {
                    scopes
                },
            }]
        })
}

pub async fn get_full_assets_for_owner<T: OwnerTokenInfo + Clone>(
    conn: &impl ConnectionTrait,
    assets: Vec<T>,
    options: &Options,
) -> Result<Vec<FullAsset>, DbErr>
where
    asset::Model: From<T>,
{
    // Using IndexMap to preserve order.
    let (mut assets_map, mut assets_with_data_map, editions_map) = assets.clone().into_iter().fold(
        (
            IndexMap::<Vec<u8>, FullAsset>::new(),
            IndexMap::<Vec<u8>, FullAsset>::new(),
            IndexMap::<Vec<u8>, Vec<u8>>::new(),
        ),
        |(mut fun_acc, mut data_acc, mut editions_map), owner_asset| {
            let id = owner_asset.get_mint().clone();
            let asset: asset::Model = owner_asset.clone().into();
            let token_info = if owner_asset.get_price().is_some()
                || owner_asset.get_decimals().is_some()
                || owner_asset.get_supply().is_some()
            {
                let total_price = match (
                    owner_asset.get_price(),
                    owner_asset.get_balance(),
                    owner_asset.get_decimals(),
                ) {
                    (Some(price), Some(balance), Some(decimals)) => {
                        let total_price = price as f64 * balance.to_f64().unwrap_or(0.0)
                            / (10u64.pow(decimals as u32)) as f64;
                        Some(total_price)
                    }
                    _ => None,
                };

                let price_info = owner_asset.get_price().map(|price| PriceInfo {
                    price_per_token: Some(price),
                    total_price,
                    currency: Some("USDC".to_string()),
                });

                let mint_token_program_pubkey = owner_asset
                    .get_token_program()
                    .as_ref()
                    .map(|pgm| Some(Pubkey::try_from(pgm.as_slice()).unwrap()))
                    .unwrap_or_else(|| None);

                let token_accounts = owner_asset.get_token_accounts().map(|accounts| {
                    accounts
                        .into_iter()
                        .map(|(address, balance)| DaoTokenAccount {
                            address: bs58::encode(address).into_string(),
                            balance,
                        })
                        .collect()
                });

                Some(TokenInfo {
                    token_accounts,
                    balance: owner_asset.get_balance().map(|b| b.to_u64().unwrap_or(0)),
                    symbol: owner_asset.get_price_symbol().clone(),
                    price_info,
                    supply: owner_asset.get_supply().map(|val| val as u64),
                    decimals: owner_asset.get_decimals(),
                    token_program: owner_asset
                        .get_token_program()
                        .clone()
                        .map(|pgm| bs58::encode(pgm).into_string()),
                    associated_token_address: find_associated_token_address(
                        Pubkey::try_from(owner_asset.get_owner().clone()).unwrap(),
                        Pubkey::try_from(owner_asset.get_mint().clone()).unwrap(),
                        mint_token_program_pubkey,
                    )
                    .map(|pubkey| pubkey.to_string())
                    .ok(),
                    mint_authority: owner_asset
                        .get_mint_authority()
                        .clone()
                        .map(|ma| bs58::encode(ma).into_string()),
                    freeze_authority: owner_asset
                        .get_freeze_authority()
                        .clone()
                        .map(|fa| bs58::encode(fa).into_string()),
                })
            } else {
                None
            };

            // Build metadata from inlined fields instead of separate query
            let asset_metadata = build_asset_metadata_from_inlined(&owner_asset);
            let has_metadata = owner_asset.get_metadata_url().is_some();

            let fa = FullAsset {
                asset: asset.clone(),
                data: asset_metadata,
                authorities: vec![],
                creators: vec![],
                groups: vec![],
                token_info,
                editions: None,
                group_definition: None,
            };

            if asset.asset_data.is_some() && has_metadata {
                data_acc.insert(id.clone(), fa.clone());
            } else {
                fun_acc.insert(id.clone(), fa.clone());
            }
            if let Some(edition_address) = fa.asset.edition_address {
                editions_map.insert(edition_address, id);
            }

            (fun_acc, data_acc, editions_map)
        },
    );

    for asset_full in assets_with_data_map.values_mut() {
        asset_full.authorities = get_asset_authorities(&asset_full.asset);
    }

    let edition_addresses = editions_map
        .clone()
        .into_iter()
        .map(|(key, _)| key)
        .collect::<Vec<Vec<u8>>>();

    let editions = get_related_editions(conn, edition_addresses).await?;

    for edition in editions.into_iter() {
        if let Ok(decoded_address) = bs58::decode(edition.address.clone()).into_vec() {
            if let Some(asset_id) = editions_map.get(&decoded_address) {
                if let Some(asset) = assets_map.get_mut(asset_id) {
                    asset.editions = Some(edition);
                } else if let Some(asset) = assets_with_data_map.get_mut(asset_id) {
                    asset.editions = Some(edition);
                }
            }
        }
    }

    // Parse creators from JSONB, fall back to database query if JSONB is NULL
    let ids_with_data = assets_with_data_map.keys().cloned().collect::<Vec<_>>();
    let assets_missing_creators: Vec<Vec<u8>> = assets_with_data_map
        .iter()
        .filter(|(_, asset)| asset.asset.creators_info.is_none())
        .map(|(id, _)| id.clone())
        .collect();

    if assets_missing_creators.is_empty() {
        // All assets have creators_info JSONB - use optimized path
        for (_id, asset) in assets_with_data_map.iter_mut() {
            let mut creators = parse_creators_from_jsonb(&asset.asset.id, &asset.asset.creators_info);
            filter_out_stale_creators(&mut creators);
            asset.creators = creators;
        }
    } else {
        // Some assets missing creators_info - fall back to database query
        let creators: Vec<asset_creators::Model> = asset_creators::Entity::find()
            .filter(asset_creators::Column::AssetId.is_in(ids_with_data.clone()))
            .order_by_asc(asset_creators::Column::AssetId)
            .order_by_asc(asset_creators::Column::Position)
            .all(conn)
            .await?;
        for c in creators.into_iter() {
            if let Some(asset) = assets_with_data_map.get_mut(&c.asset_id) {
                asset.creators.push(c);
            }
        }
        for asset in assets_with_data_map.values_mut() {
            filter_out_stale_creators(&mut asset.creators);
        }
    }

    // Extract groupings from JSONB instead of separate query
    let (groups, group_definitions) = extract_groupings_from_assets(
        &assets_with_data_map,
        options.show_unverified_collections,
    );

    for g in groups.into_iter() {
        if let Some(asset) = assets_with_data_map.get_mut(&g.asset_id) {
            asset.groups.push(g);
        }
    }

    for g in group_definitions.into_iter() {
        if let Some(asset) = assets_with_data_map.get_mut(&g.asset_id) {
            asset.group_definition = Some(g);
        }
    }

    let mut final_assets_list = Vec::new();

    for asset in assets {
        if let Some(asset) = assets_with_data_map.get(asset.get_mint()) {
            final_assets_list.push(asset.clone());
        } else if let Some(asset) = assets_map.get(asset.get_mint()) {
            final_assets_list.push(asset.clone());
        } else {
            error!("Asset not found in either map: {:?}", asset.get_mint());
        }
    }

    Ok(final_assets_list)
}

// TODO: If an asset is missing asset_data, it will be skipped in the response which leads to customer confusion.
// For metaplex tokens, this happens when the asset is burnt or the token is malformed (no metadata created yet).
// We need to figure out how to handle these assets. Do we skip them? If so, it has to be clear to the customer.
//
// My opinion is that we should filter out these records in the initial query.
// If asset_data is missing here, then that's acceptable by design (e.g. they searched for ALL tokens) and we should show it.
pub async fn get_related_for_assets(
    conn: &impl ConnectionTrait,
    assets: Vec<asset::Model>,
    options: &Options,
) -> Result<Vec<FullAsset>, DbErr> {
    let asset_ids = assets.iter().map(|a| a.id.clone()).collect::<Vec<_>>();
    let asset_metadata = asset_data_v2::Entity::find()
        .find_also_related(offchain_metadata::Entity)
        .filter(asset_data_v2::Column::Id.is_in(asset_ids))
        .all(conn)
        .await?;
    let asset_data_map = asset_metadata
        .into_iter()
        .fold(HashMap::new(), |mut acc, ad| {
            let asset_data = ad.0;
            let metadata = ad.1;
            let asset_metadata = match metadata {
                Some(m) => AssetMetadata {
                    id: asset_data.id,
                    metadata_url: asset_data.metadata_url,
                    chain_data: asset_data.chain_data,
                    chain_mutability: asset_data.chain_mutability,
                    raw_name: asset_data.raw_name,
                    raw_symbol: asset_data.raw_symbol,
                    metadata: m.metadata,
                },
                None => AssetMetadata::default(),
            };
            acc.insert(asset_metadata.id.clone(), asset_metadata);
            acc
        });

    // Using IndexMap to preserve order.
    let (mut assets_map, editions_map) = assets.into_iter().fold(
        (IndexMap::new(), IndexMap::new()),
        |(mut acc, mut editions_map), asset| {
            if let Some(ad) = asset
                .asset_data
                .clone()
                .and_then(|ad_id| asset_data_map.get(&ad_id))
            {
                let id = asset.id.clone();
                let fa = FullAsset {
                    asset,
                    data: ad.clone(),
                    authorities: vec![],
                    creators: vec![],
                    groups: vec![],
                    token_info: None,
                    editions: None,
                    group_definition: None,
                };
                acc.insert(id.clone(), fa.clone());
                if let Some(edition_address) = fa.asset.edition_address {
                    editions_map.insert(edition_address, id);
                }
            }
            (acc, editions_map)
        },
    );

    for asset_full in assets_map.values_mut() {
        asset_full.authorities = get_asset_authorities(&asset_full.asset);
    }
    let ids = assets_map.keys().cloned().collect::<Vec<_>>();

    // Parse creators from JSONB, fall back to database query if JSONB is NULL
    let assets_missing_creators: Vec<Vec<u8>> = assets_map
        .iter()
        .filter(|(_, asset)| asset.asset.creators_info.is_none())
        .map(|(id, _)| id.clone())
        .collect();

    if assets_missing_creators.is_empty() {
        // All assets have creators_info JSONB - use optimized path
        for (_id, asset) in assets_map.iter_mut() {
            let mut creators = parse_creators_from_jsonb(&asset.asset.id, &asset.asset.creators_info);
            filter_out_stale_creators(&mut creators);
            asset.creators = creators;
        }
    } else {
        // Some assets missing creators_info - fall back to database query
        let creators = asset_creators::Entity::find()
            .filter(asset_creators::Column::AssetId.is_in(ids.clone()))
            .order_by_asc(asset_creators::Column::AssetId)
            .order_by_asc(asset_creators::Column::Position)
            .all(conn)
            .await?;
        for c in creators.into_iter() {
            if let Some(asset) = assets_map.get_mut(&c.asset_id) {
                asset.creators.push(c);
            }
        }
        for asset in assets_map.values_mut() {
            filter_out_stale_creators(&mut asset.creators);
        }
    }

    // Extract groupings from JSONB instead of separate query
    let (groups, group_definitions) = extract_groupings_from_assets(
        &assets_map,
        options.show_unverified_collections,
    );

    let edition_addresses = editions_map
        .clone()
        .into_iter()
        .map(|(key, _)| key)
        .collect::<Vec<Vec<u8>>>();

    let editions = get_related_editions(conn, edition_addresses).await?;

    for edition in editions.into_iter() {
        if let Ok(decoded_address) = bs58::decode(edition.address.clone()).into_vec() {
            if let Some(asset_id) = editions_map.get(&decoded_address) {
                if let Some(asset) = assets_map.get_mut(asset_id) {
                    asset.editions = Some(edition);
                }
            }
        }
    }

    let mints = tokens::Entity::find()
        .filter(tokens::Column::Mint.is_in(ids.clone()))
        .order_by_asc(tokens::Column::Mint)
        .all(conn)
        .await?;
    for mint in mints.into_iter() {
        if let Some(asset) = assets_map.get_mut(&mint.mint) {
            let token_info = TokenInfo {
                token_accounts: None,
                supply: Some(mint.supply as u64),
                decimals: Some(mint.decimals),
                token_program: Some(bs58::encode(mint.token_program.clone()).into_string()),
                associated_token_address: match asset.asset.clone().owner {
                    None => None,
                    Some(owner) => {
                        let ata = find_associated_token_address(
                            Pubkey::try_from(owner).unwrap(),
                            Pubkey::try_from(asset.asset.clone().id).unwrap(),
                            Some(Pubkey::try_from(mint.token_program).unwrap()),
                        );
                        match ata {
                            Ok(ata) => Some(bs58::encode(ata).into_string()),
                            Err(_) => None,
                        }
                    }
                },
                ..Default::default()
            };
            asset.token_info = Some(token_info);
        }
    }

    for g in groups.into_iter() {
        if let Some(asset) = assets_map.get_mut(&g.asset_id) {
            asset.groups.push(g);
        }
    }

    for g in group_definitions.into_iter() {
        if let Some(asset) = assets_map.get_mut(&g.asset_id) {
            asset.group_definition = Some(g);
        }
    }

    Ok(assets_map.into_iter().map(|(_, v)| v).collect())
}

pub async fn get_assets_by_condition(
    conn: &impl ConnectionTrait,
    condition: Condition,
    joins: Vec<RelationDef>,
    sort_by: Option<asset::Column>,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
    options: &Options,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let mut stmt = asset::Entity::find();
    for def in joins {
        stmt = stmt.join(JoinType::LeftJoin, def);
    }
    stmt = stmt.filter(condition);
    if let Some(col) = sort_by {
        stmt = stmt
            .order_by(col, sort_direction.clone())
            .order_by(asset::Column::Id, sort_direction.clone());
    }

    let (assets, grand_total) = get_full_response(
        conn,
        stmt,
        pagination,
        limit,
        sort_by,
        sort_direction,
        enable_grand_total_query,
        options,
    )
    .await?;
    Ok((assets, grand_total))
}

impl From<tokens::Model> for asset::Model {
    fn from(token: tokens::Model) -> Self {
        asset::Model {
            id: token.mint.clone(),
            alt_id: None,
            supply: token.supply,
            supply_mint: Some(token.mint.clone()),
            slot_updated_mint_account: Some(token.slot_updated),
            mint_extensions: token.extensions.clone(),
            token_extensions: token.extensions,
            specification_version: Some(SpecificationVersions::V1),
            specification_asset_class: Some(SpecificationAssetClass::Unknown),
            owner_type: OwnerType::Token,
            ..Default::default()
        }
    }
}

pub async fn get_by_mint(
    conn: &impl ConnectionTrait,
    asset_id: Vec<u8>,
    include_no_supply: bool,
    _options: &Options,
) -> Result<FullAsset, DbErr> {
    let mut stmt = tokens::Entity::find_by_id(asset_id.clone()).find_also_related(price::Entity);
    if !include_no_supply {
        stmt = stmt.filter(tokens::Column::Supply.ne(0));
    }

    let mint_data: Result<(tokens::Model, Option<price::Model>), DbErr> =
        stmt.one(conn).await.and_then(|o| match o {
            Some((a, Some(d))) => Ok((a, Some(d))),
            Some((a, None)) => Ok((a, None)),
            _ => Err(DbErr::RecordNotFound("Asset Not Found".to_string())),
        });

    let (mint, price) = mint_data?;

    let price_info = if let Some(price) = price.clone() {
        Some(PriceInfo {
            price_per_token: price.price,
            currency: Some("USDC".to_string()),
            total_price: None,
        })
    } else {
        None
    };

    let token_info = TokenInfo {
        token_accounts: None,
        symbol: price.map(|price| price.symbol.unwrap_or_default()),
        price_info,
        supply: Some(mint.supply as u64),
        decimals: Some(mint.decimals),
        balance: None,
        associated_token_address: None,
        token_program: Some(bs58::encode(mint.token_program.clone()).into_string()),
        mint_authority: mint
            .mint_authority
            .as_ref()
            .map(|auth| bs58::encode(auth).into_string()),
        freeze_authority: mint
            .freeze_authority
            .as_ref()
            .map(|auth| bs58::encode(auth).into_string()),
    };

    Ok(FullAsset {
        asset: asset::Model::from(mint),
        data: AssetMetadata::default(),
        authorities: vec![],
        creators: vec![],
        groups: vec![],
        token_info: Some(token_info),
        editions: None,
        group_definition: None,
    })
}

pub async fn get_by_asset_id(
    conn: &impl ConnectionTrait,
    asset_id: Vec<u8>,
    include_no_supply: bool,
    options: &Options,
    verify_asset_exists: bool,
) -> Result<FullAsset, DbErr> {
    let mut cond = Condition::all().add(asset::Column::Id.eq(asset_id.clone()));
    if !include_no_supply {
        cond = cond.add(asset::Column::Supply.ne(0));
    }
    // Dirty fix for Tensor.
    // Collections info is the last thing to update during cNFT mint (mint_v1.rs).
    // We check that it is not null. If it's not null, then the asset is fully indexed.
    if options.require_full_index {
        cond = cond.add(asset::Column::CollectionsInfo.is_not_null());
    }
    let (asset, data) = get_asset_and_metadata(conn, cond, verify_asset_exists).await?;
    let authorities = get_asset_authorities(&asset);

    let mut creators: Vec<asset_creators::Model> = asset_creators::Entity::find()
        .filter(asset_creators::Column::AssetId.eq(asset.id.clone()))
        .order_by_asc(asset_creators::Column::Position)
        .all(conn)
        .await?;
    filter_out_stale_creators(&mut creators);

    let (mut groups, group_definition) =
        get_related_groupings(conn, vec![asset_id], options.show_unverified_collections).await?;

    // Also extract MPL Core group memberships from the already-loaded asset model.
    // get_related_groupings can miss these because its query filters by collections_info
    // verification status, which excludes assets whose only grouping is via the Groups plugin.
    groups.extend(extract_mpl_core_group_memberships(&asset));

    let mut editions = None;
    if let Some(edition_address) = asset.edition_address.clone() {
        editions = get_related_edition(conn, edition_address).await?;
    }

    Ok(FullAsset {
        asset,
        data,
        authorities,
        creators,
        groups,
        token_info: None,
        editions,
        group_definition: if group_definition.is_empty() {
            None
        } else {
            Some(group_definition[0].clone())
        },
    })
}

pub async fn get_by_id(
    conn: &impl ConnectionTrait,
    asset_id: Vec<u8>,
    include_no_supply: bool,
    options: &Options,
    verify_asset_exists: bool,
) -> Result<FullAsset, DbErr> {
    let get_by_asset_future = get_by_asset_id(
        conn,
        asset_id.clone(),
        include_no_supply,
        options,
        verify_asset_exists,
    );
    let get_by_mint_future = get_by_mint(conn, asset_id, include_no_supply, options);
    let (asset_result, mint_result) = tokio::join!(get_by_asset_future, get_by_mint_future);
    match (asset_result, mint_result) {
        (Ok(mut asset), Ok(mint)) => {
            asset.token_info = mint.token_info;
            if asset.asset.mint_extensions.is_none() {
                asset.asset.mint_extensions = mint.asset.mint_extensions;
            }
            Ok(asset)
        }
        (Ok(asset), Err(_)) => Ok(asset),
        (Err(_), Ok(mint)) => Ok(mint),
        (Err(e), Err(_)) => Err(e),
    }
}

pub async fn fetch_transactions(
    conn: &impl ConnectionTrait,
    tree: Vec<u8>,
    leaf_idx: i64,
    pagination: &Pagination,
    limit: u64,
    sort_direction: Option<AssetSortDirection>,
) -> Result<Vec<(String, String)>, DbErr> {
    // Default sort direction is Desc
    // Similar to GetSignaturesForAddress in the Solana API
    let sort_direction = sort_direction.unwrap_or(AssetSortDirection::Asc);
    let sort_order = match sort_direction {
        AssetSortDirection::Asc => sea_orm::Order::Asc,
        AssetSortDirection::Desc => sea_orm::Order::Desc,
    };

    let mut stmt = cl_audits_v2::Entity::find().filter(cl_audits_v2::Column::Tree.eq(tree));
    stmt = stmt.filter(cl_audits_v2::Column::LeafIdx.eq(leaf_idx));
    stmt = stmt.order_by(cl_audits_v2::Column::Seq, sort_order.clone());

    stmt = paginate(
        pagination,
        limit,
        stmt,
        sort_order,
        cl_audits_v2::Column::Seq,
    );
    let transactions = stmt.all(conn).await?;
    let transaction_list = transactions
        .into_iter()
        .map(|transaction| {
            let tx = bs58::encode(transaction.tx).into_string();
            let ix = Instruction::to_pascal_case(&transaction.instruction);
            (tx, ix)
        })
        .collect();

    Ok(transaction_list)
}

pub async fn get_asset_signatures(
    conn: &impl ConnectionTrait,
    asset_id: Option<Vec<u8>>,
    tree_id: Option<Vec<u8>>,
    leaf_idx: Option<i64>,
    pagination: &Pagination,
    limit: u64,
    sort_direction: Option<AssetSortDirection>,
) -> Result<Vec<(String, String)>, DbErr> {
    // if tree_id and leaf_idx are provided, use them directly to fetch transactions
    if let (Some(tree_id), Some(leaf_idx)) = (tree_id, leaf_idx) {
        let transactions =
            fetch_transactions(conn, tree_id, leaf_idx, pagination, limit, sort_direction).await?;
        return Ok(transactions);
    }

    if asset_id.is_none() {
        return Err(DbErr::Custom(
            "Either 'id' or both 'tree' and 'leafIndex' must be provided".to_string(),
        ));
    }

    // if only asset_id is provided, fetch the latest tree and leaf_idx (asset.nonce) for the asset
    // and use them to fetch transactions
    let stmt = asset::Entity::find()
        .distinct_on([(asset::Entity, asset::Column::Id)])
        .filter(asset::Column::Id.eq(asset_id))
        .order_by(asset::Column::Id, Order::Desc)
        .limit(1);
    let asset = stmt.one(conn).await?;
    if let Some(asset) = asset {
        let tree = asset
            .tree_id
            .ok_or(DbErr::RecordNotFound("Tree not found".to_string()))?;
        if tree.is_empty() {
            return Err(DbErr::Custom("Empty tree for asset".to_string()));
        }
        let leaf_idx = asset
            .nonce
            .ok_or(DbErr::RecordNotFound("Leaf ID does not exist".to_string()))?;
        let transactions =
            fetch_transactions(conn, tree, leaf_idx, pagination, limit, sort_direction).await?;
        Ok(transactions)
    } else {
        Ok(Vec::new())
    }
}

async fn get_full_response(
    conn: &impl ConnectionTrait,
    stmt: Select<Entity>,
    pagination: &Pagination,
    limit: u64,
    _sort_by: Option<asset::Column>,
    sort_direction: Order,
    enable_grand_total_query: bool,
    options: &Options,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    if enable_grand_total_query {
        let grand_total_task = get_grand_total(conn, stmt.clone());
        let assets_task =
            paginate(pagination, limit, stmt, sort_direction, asset::Column::Id).all(conn);

        let (assets, grand_total) = try_join!(assets_task, grand_total_task)?;
        let full_assets = get_related_for_assets(conn, assets, options).await?;
        return Ok((full_assets, grand_total));
    }
    let assets = paginate(pagination, limit, stmt, sort_direction, asset::Column::Id)
        .all(conn)
        .await?;
    let full_assets = get_related_for_assets(conn, assets, options).await?;
    Ok((full_assets, None))
}

async fn get_grand_total(
    conn: &impl ConnectionTrait,
    stmt: Select<Entity>,
) -> Result<Option<u64>, DbErr> {
    let grand_total = stmt.count(conn).await?;
    Ok(Some(grand_total))
}

pub async fn add_collection_metadata(
    conn: &impl ConnectionTrait,
    assets: &mut Vec<Asset>,
) -> Result<(), DbErr> {
    // compile a set of all the distinct group values (bs58 String) from the asset list
    let mut group_values: HashSet<String> = HashSet::new();
    for asset in assets.iter() {
        if let Some(groups) = &asset.grouping {
            for group in groups {
                if let Some(group_value) = &group.group_value {
                    group_values.insert(group_value.clone());
                }
            }
        }
    }

    // convert the group values to bytea by decoding them from bs58
    let bytea_group_values: Vec<Vec<u8>> = group_values
        .iter()
        .map(|group_value| {
            let bs58_decoded = bs58::decode(group_value).into_vec().unwrap_or_default();
            bs58_decoded
        })
        .collect();

    let asset_metadata = asset_data_v2::Entity::find()
        .find_also_related(offchain_metadata::Entity)
        .filter(Condition::all().add(asset_data_v2::Column::Id.is_in(bytea_group_values)))
        .all(conn)
        .await?;
    let asset_metadata = asset_metadata.into_iter().fold(Vec::new(), |mut acc, ad| {
        let asset_data = ad.0;
        let metadata = ad.1;
        if let Some(m) = metadata {
            let asset_metadata = AssetMetadata {
                id: asset_data.id,
                metadata_url: asset_data.metadata_url,
                chain_data: asset_data.chain_data,
                chain_mutability: asset_data.chain_mutability,
                raw_name: asset_data.raw_name,
                raw_symbol: asset_data.raw_symbol,
                metadata: m.metadata,
            };
            acc.push(asset_metadata);
        }
        acc
    });

    // create a mapping of id -> collection_metadata
    let mut hashmap: HashMap<String, CollectionMetadata> = HashMap::new();
    for data in &asset_metadata {
        let id = bs58::encode(&data.id).into_string();
        let collection_metadata = get_collection_metadata(&data);
        hashmap.insert(id, collection_metadata);
    }

    // add the metadata to the asset_list
    for asset in assets.iter_mut() {
        if let Some(groups) = &mut asset.grouping {
            for group in groups {
                if let Some(group_value) = &group.group_value {
                    let collection_metadata = hashmap.get(group_value);
                    if let Some(collection_metadata) = collection_metadata {
                        group.collection_metadata = Some(collection_metadata.clone());
                    }
                }
            }
        }
    }

    Ok(())
}

fn get_collection_metadata(data: &AssetMetadata) -> CollectionMetadata {
    let chain_data_selector = &mut jsonpath_lib::selector(&data.chain_data);
    let metadata_selector = &mut jsonpath_lib::selector(&data.metadata);

    let name = safe_select(chain_data_selector, "$.name");
    let symbol = safe_select(chain_data_selector, "$.symbol");
    let image = safe_select(metadata_selector, "$.image");
    let description = safe_select(metadata_selector, "$.description");
    let external_url = safe_select(metadata_selector, "$.external_url");

    let col_metadata_name = name
        .map(|n| n.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let col_metadata_symbol = symbol
        .map(|s| s.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let col_metadata_image = image
        .map(|i| i.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let col_metadata_description = description
        .map(|d| d.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let col_metadata_external_url = external_url
        .map(|u| u.to_string().trim_matches('"').to_string())
        .unwrap_or_default();

    CollectionMetadata {
        name: Some(col_metadata_name),
        symbol: Some(col_metadata_symbol),
        image: Some(col_metadata_image),
        description: Some(col_metadata_description),
        external_url: Some(col_metadata_external_url),
    }
}

async fn get_related_groupings(
    conn: &impl ConnectionTrait,
    asset_ids: Vec<Vec<u8>>,
    show_unverified_collections: bool,
) -> Result<(Vec<Group>, Vec<GroupDefinition>), DbErr> {
    let mut condition = Condition::all().add(asset::Column::Id.is_in(asset_ids));
    condition = condition.add(SimpleExpr::Custom(
        "collections_info IS NOT NULL AND collections_info != '{}'".into(),
    ));

    if !show_unverified_collections {
        let verified_expr = SimpleExpr::Custom("collections_info @> '{\"verified\": true}'".into());
        let null_string_expr =
            SimpleExpr::Custom("(collections_info ->> 'verified')::text IS NULL".into());
        let null_expr = SimpleExpr::Custom("collections_info -> 'verified' IS NULL".into());

        condition = condition.add(verified_expr.or(null_string_expr).or(null_expr));
    }

    let assets = asset::Entity::find()
        .filter(condition)
        .order_by_asc(asset::Column::Id)
        .all(conn)
        .await?;

    let grouping: Vec<Group> = assets
        .iter()
        .filter_map(|asset| {
            if let Some(collections_info) = &asset.collections_info {
                collections_info.as_object().map(|info| Group {
                    asset_id: asset.id.clone(),
                    group_key: "collection".to_string(),
                    group_value: info
                        .get("collection_id")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    verified: info.get("verified").and_then(|v| v.as_bool()),
                    collection_metadata: None,
                })
            } else {
                None
            }
        })
        .collect();

    let group_def: Vec<GroupDefinition> = assets
        .iter()
        .filter_map(|asset| {
            asset
                .collections_info
                .as_ref()
                .and_then(|collections_info| {
                    let info = collections_info.as_object()?;
                    match info.get("collection_nft").and_then(|v| v.as_bool()) {
                        Some(true) => Some(GroupDefinition {
                            asset_id: asset.id.clone(),
                            group_key: "collection".to_string(),
                            group_value: Some(bs58::encode(asset.id.clone()).into_string()),
                            size: info.get("collection_size").and_then(|v| v.as_u64()),
                        }),
                        _ => None,
                    }
                })
        })
        .collect();

    Ok((grouping, group_def))
}

fn filter_out_stale_creators(creators: &mut Vec<asset_creators::Model>) {
    // If the first creator is an empty Vec, it means the creator array is empty (which is allowed
    // for compressed assets in Bubblegum).
    if !creators.is_empty() && creators[0].creator.is_empty() {
        creators.clear();
    } else {
        // For both compressed and non-compressed assets, any creators that do not have the max
        // `slot_updated` value are stale and should be removed.
        let max_slot_updated = creators.iter().map(|creator| creator.slot_updated).max();
        if let Some(max_slot_updated) = max_slot_updated {
            creators.retain(|creator| creator.slot_updated == max_slot_updated);
        }

        // For compressed assets, any creators that do not have the max `seq` value are stale and
        // should be removed.  A `seq` value of 0 indicates a decompressed or never-compressed
        // asset.  So if a `seq` value of 0 is present, then all creators with nonzero `seq` values
        // are stale and should be removed.
        let seq = if creators
            .iter()
            .map(|creator| creator.seq)
            .any(|seq| seq == Some(0))
        {
            Some(Some(0))
        } else {
            creators.iter().map(|creator| creator.seq).max()
        };

        if let Some(seq) = seq {
            creators.retain(|creator| creator.seq == seq);
        }
    }
}

/// Extract MPL Core group memberships from `collections_info.groups`.
fn extract_mpl_core_group_memberships(asset: &asset::Model) -> Vec<Group> {
    let from_collections_info = asset
        .collections_info
        .as_ref()
        .and_then(|info| info.get("groups"))
        .and_then(|g| g.as_array());

    // Pre-backfill fallback: rows indexed before this change lack the `groups`
    // key, so read the Groups plugin as the prior fix did. Remove after backfill.
    let group_addresses = match from_collections_info {
        Some(arr) => arr,
        None => match asset
            .mpl_core_plugins
            .as_ref()
            .and_then(|plugins| plugins.get("groups"))
            .and_then(|g| g.get("data"))
            .and_then(|d| d.get("groups"))
            .and_then(|g| g.as_array())
        {
            Some(arr) => arr,
            None => return vec![],
        },
    };

    group_addresses
        .iter()
        .filter_map(|addr| {
            addr.as_str().map(|s| Group {
                asset_id: asset.id.clone(),
                group_key: "group".to_string(),
                group_value: Some(s.to_string()),
                verified: None,
                collection_metadata: None,
            })
        })
        .collect()
}

/// Extract groupings from collections_info JSONB column.
fn extract_groupings_from_assets(
    assets: &IndexMap<Vec<u8>, FullAsset>,
    show_unverified_collections: bool,
) -> (Vec<Group>, Vec<GroupDefinition>) {
    let mut groups = Vec::new();
    let mut group_definitions = Vec::new();

    for (_, full_asset) in assets.iter() {
        let asset = &full_asset.asset;

        // Extract before the collection checks below, which may `continue`.
        groups.extend(extract_mpl_core_group_memberships(asset));

        if let Some(collections_info) = &asset.collections_info {
            if let Some(info) = collections_info.as_object() {
                // Check if collections_info is not empty
                if info.is_empty() {
                    continue;
                }

                // Check verified status if show_unverified_collections is false
                let verified = info.get("verified").and_then(|v| v.as_bool());
                if !show_unverified_collections {
                    // Allow: verified=true, verified=null, or missing verified field
                    if verified == Some(false) {
                        continue;
                    }
                }

                // Create Group
                groups.push(Group {
                    asset_id: asset.id.clone(),
                    group_key: "collection".to_string(),
                    group_value: info
                        .get("collection_id")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    verified,
                    collection_metadata: None,
                });

                // Create GroupDefinition if collection_nft is true
                if info.get("collection_nft").and_then(|v| v.as_bool()) == Some(true) {
                    group_definitions.push(GroupDefinition {
                        asset_id: asset.id.clone(),
                        group_key: "collection".to_string(),
                        group_value: Some(bs58::encode(asset.id.clone()).into_string()),
                        size: info.get("collection_size").and_then(|v| v.as_u64()),
                    });
                }
            }
        }
    }

    (groups, group_definitions)
}

/// Build AssetMetadata from inlined query fields.
fn build_asset_metadata_from_inlined<T: OwnerTokenInfo>(owner_asset: &T) -> AssetMetadata {
    use crate::dao::sea_orm_active_enums::ChainMutability;

    let id = owner_asset
        .get_mint()
        .clone();

    match owner_asset.get_metadata_url() {
        Some(url) => AssetMetadata {
            id,
            metadata_url: url.clone(),
            chain_data: owner_asset.get_chain_data().clone().unwrap_or(serde_json::Value::Null),
            chain_mutability: owner_asset.get_chain_mutability().clone().unwrap_or(ChainMutability::Mutable),
            raw_name: owner_asset.get_raw_name().clone(),
            raw_symbol: owner_asset.get_raw_symbol().clone(),
            metadata: owner_asset.get_offchain_metadata().clone().unwrap_or(serde_json::Value::Null),
        },
        None => AssetMetadata::default(),
    }
}

/// Parse creators from the creators_info JSONB column.
fn parse_creators_from_jsonb(
    asset_id: &[u8],
    creators_info: &Option<serde_json::Value>,
) -> Vec<asset_creators::Model> {
    let Some(info) = creators_info else {
        return vec![];
    };

    let seq = info.get("seq").and_then(|v| v.as_i64());
    let slot_updated = info.get("slot_updated").and_then(|v| v.as_i64());

    let Some(creators_array) = info.get("creators").and_then(|v| v.as_array()) else {
        return vec![];
    };

    creators_array
        .iter()
        .enumerate()
        .filter_map(|(position, creator_obj)| {
            let creator_bytes: Vec<u8> = creator_obj
                .get("creator")
                .and_then(|v| v.as_array())?
                .iter()
                .filter_map(|b| b.as_u64().map(|n| n as u8))
                .collect();

            let share = creator_obj.get("share").and_then(|v| v.as_i64())? as i32;
            let verified = creator_obj
                .get("verified")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            Some(asset_creators::Model {
                id: 0, // Not used in response serialization
                asset_id: asset_id.to_vec(),
                creator: creator_bytes,
                share,
                verified,
                seq,
                slot_updated,
                position: position as i16,
            })
        })
        .collect()
}
