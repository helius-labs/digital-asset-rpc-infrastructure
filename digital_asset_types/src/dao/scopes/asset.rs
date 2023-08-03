use crate::{
    dao::{
        asset::{self, Entity},
        asset_authority, asset_creators, asset_data, asset_grouping, cl_audits, FullAsset,
        GroupingSize, Pagination,
    },
    dapi::common::safe_select,
    rpc::{response::AssetList, CollectionMetadata},
};

use indexmap::IndexMap;
use sea_orm::{entity::*, query::*, ConnectionTrait, DbErr, Order};
use std::collections::{HashMap, HashSet};
use tokio::try_join;

pub fn paginate<'db, T>(pagination: &Pagination, limit: u64, stmt: T) -> T
where
    T: QueryFilter + QuerySelect,
{
    let mut stmt = stmt;
    match pagination {
        Pagination::Keyset { before, after } => {
            if let Some(b) = before {
                stmt = stmt.filter(asset::Column::Id.lt(b.clone()));
            }
            if let Some(a) = after {
                stmt = stmt.filter(asset::Column::Id.gt(a.clone()));
            }
        }
        Pagination::Page { page } => {
            if *page > 0 {
                stmt = stmt.offset((page - 1) * limit)
            }
        }
    }
    stmt.limit(limit)
}

pub async fn get_by_creator(
    conn: &impl ConnectionTrait,
    creator: Vec<u8>,
    only_verified: bool,
    sort_by: asset::Column,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let mut condition = Condition::all()
        .add(asset_creators::Column::Creator.eq(creator))
        .add(asset::Column::Supply.gt(0));
    if only_verified {
        condition = condition.add(asset_creators::Column::Verified.eq(true));
    }
    get_by_related_condition(
        conn,
        condition,
        asset::Relation::AssetCreators,
        sort_by,
        sort_direction,
        pagination,
        limit,
        enable_grand_total_query,
    )
    .await
}

pub async fn get_grouping(
    conn: &impl ConnectionTrait,
    group_key: String,
    group_value: String,
) -> Result<GroupingSize, DbErr> {
    let size = asset_grouping::Entity::find()
        .filter(
            Condition::all()
                .add(asset_grouping::Column::GroupKey.eq(group_key))
                .add(asset_grouping::Column::GroupValue.eq(group_value)),
        )
        .count(conn)
        .await?;
    Ok(GroupingSize { size })
}

pub async fn get_by_grouping(
    conn: &impl ConnectionTrait,
    group_key: String,
    group_value: String,
    sort_by: asset::Column,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let condition = asset_grouping::Column::GroupKey
        .eq(group_key)
        .and(asset_grouping::Column::GroupValue.eq(group_value));
    get_by_related_condition(
        conn,
        Condition::all()
            .add(condition)
            .add(asset::Column::Supply.gt(0)),
        asset::Relation::AssetGrouping,
        sort_by,
        sort_direction,
        pagination,
        limit,
        enable_grand_total_query,
    )
    .await
}

pub async fn get_assets_by_owner(
    conn: &impl ConnectionTrait,
    owner: Vec<u8>,
    sort_by: asset::Column,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
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
    )
    .await
}

pub async fn get_by_authority(
    conn: &impl ConnectionTrait,
    authority: Vec<u8>,
    sort_by: asset::Column,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let cond = Condition::all()
        .add(asset_authority::Column::Authority.eq(authority))
        .add(asset::Column::Supply.gt(0));
    get_by_related_condition(
        conn,
        cond,
        asset::Relation::AssetAuthority,
        sort_by,
        sort_direction,
        pagination,
        limit,
        enable_grand_total_query,
    )
    .await
}

async fn get_by_related_condition<E>(
    conn: &impl ConnectionTrait,
    condition: Condition,
    relation: E,
    sort_by: asset::Column,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr>
where
    E: RelationTrait,
{
    let stmt = asset::Entity::find()
        .filter(condition)
        .join(JoinType::LeftJoin, relation.def())
        .order_by(sort_by, sort_direction.clone())
        .order_by(asset::Column::Id, sort_direction);

    let (assets, grand_total) =
        get_full_response(conn, stmt, pagination, limit, enable_grand_total_query).await?;
    Ok((assets, grand_total))
}

pub async fn get_related_for_assets(
    conn: &impl ConnectionTrait,
    assets: Vec<asset::Model>,
) -> Result<Vec<FullAsset>, DbErr> {
    let asset_ids = assets.iter().map(|a| a.id.clone()).collect::<Vec<_>>();

    let asset_data: Vec<asset_data::Model> = asset_data::Entity::find()
        .filter(asset_data::Column::Id.is_in(asset_ids))
        .all(conn)
        .await?;
    let asset_data_map = asset_data.into_iter().fold(HashMap::new(), |mut acc, ad| {
        acc.insert(ad.id.clone(), ad);
        acc
    });

    // Using IndexMap to preserve order.
    let mut assets_map = assets.into_iter().fold(IndexMap::new(), |mut acc, asset| {
        if let Some(ad) = asset
            .asset_data
            .clone()
            .and_then(|ad_id| asset_data_map.get(&ad_id))
        {
            let id = asset.id.clone();
            let fa = FullAsset {
                asset: asset,
                data: ad.clone(),
                authorities: vec![],
                creators: vec![],
                groups: vec![],
            };
            acc.insert(id, fa);
        };
        acc
    });
    let ids = assets_map.keys().cloned().collect::<Vec<_>>();
    let authorities = asset_authority::Entity::find()
        .filter(asset_authority::Column::AssetId.is_in(ids.clone()))
        .order_by_asc(asset_authority::Column::AssetId)
        .all(conn)
        .await?;
    for a in authorities.into_iter() {
        if let Some(asset) = assets_map.get_mut(&a.asset_id) {
            asset.authorities.push(a);
        }
    }

    let creators = asset_creators::Entity::find()
        .filter(asset_creators::Column::AssetId.is_in(ids.clone()))
        .order_by_asc(asset_creators::Column::AssetId)
        .all(conn)
        .await?;
    for c in creators.into_iter() {
        if let Some(asset) = assets_map.get_mut(&c.asset_id) {
            asset.creators.push(c);
        }
    }

    let grouping = asset_grouping::Entity::find()
        .filter(asset_grouping::Column::AssetId.is_in(ids.clone()))
        .filter(asset_grouping::Column::GroupValue.is_not_null())
        .order_by_asc(asset_grouping::Column::AssetId)
        .all(conn)
        .await?;
    for g in grouping.into_iter() {
        if let Some(asset) = assets_map.get_mut(&g.asset_id) {
            asset.groups.push(g);
        }
    }

    Ok(assets_map.into_iter().map(|(_, v)| v).collect())
}

pub async fn get_assets_by_condition(
    conn: &impl ConnectionTrait,
    condition: Condition,
    joins: Vec<RelationDef>,
    sort_by: asset::Column,
    sort_direction: Order,
    pagination: &Pagination,
    limit: u64,
    enable_grand_total_query: bool,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    let mut stmt = asset::Entity::find();
    for def in joins {
        stmt = stmt.join(JoinType::LeftJoin, def);
    }
    stmt = stmt
        .filter(condition)
        .order_by(sort_by, sort_direction.clone())
        .order_by(asset::Column::Id, sort_direction);

    let (assets, grand_total) =
        get_full_response(conn, stmt, pagination, limit, enable_grand_total_query).await?;
    Ok((assets, grand_total))
}

pub async fn get_by_id(
    conn: &impl ConnectionTrait,
    asset_id: Vec<u8>,
    include_no_supply: bool,
) -> Result<FullAsset, DbErr> {
    let mut asset_data = asset::Entity::find_by_id(asset_id).find_also_related(asset_data::Entity);
    if !include_no_supply {
        asset_data = asset_data.filter(Condition::all().add(asset::Column::Supply.gt(0)));
    }

    let asset_data: (asset::Model, asset_data::Model) =
        asset_data.one(conn).await.and_then(|o| match o {
            Some((a, Some(d))) => Ok((a, d)),
            _ => Err(DbErr::RecordNotFound("Asset Not Found".to_string())),
        })?;

    let (asset, data) = asset_data;
    let authorities: Vec<asset_authority::Model> = asset_authority::Entity::find()
        .filter(asset_authority::Column::AssetId.eq(asset.id.clone()))
        .order_by_asc(asset_authority::Column::AssetId)
        .all(conn)
        .await?;
    let creators: Vec<asset_creators::Model> = asset_creators::Entity::find()
        .filter(asset_creators::Column::AssetId.eq(asset.id.clone()))
        .order_by_asc(asset_creators::Column::AssetId)
        .all(conn)
        .await?;
    let grouping: Vec<asset_grouping::Model> = asset_grouping::Entity::find()
        .filter(asset_grouping::Column::AssetId.eq(asset.id.clone()))
        .filter(asset_grouping::Column::GroupValue.is_not_null())
        .order_by_asc(asset_grouping::Column::AssetId)
        .all(conn)
        .await?;
    Ok(FullAsset {
        asset,
        data,
        authorities,
        creators,
        groups: grouping,
    })
}

pub async fn fetch_transactions(
    conn: &impl ConnectionTrait,
    tree: Vec<u8>,
    leaf_id: i64,
    pagination: &Pagination,
    limit: u64,
) -> Result<Vec<Vec<String>>, DbErr> {
    let mut stmt = cl_audits::Entity::find()
        .filter(cl_audits::Column::Tree.eq(tree))
        .filter(cl_audits::Column::LeafIdx.eq(leaf_id))
        .order_by(cl_audits::Column::CreatedAt, sea_orm::Order::Desc);

    stmt = paginate(pagination, limit, stmt);
    let transactions = stmt.all(conn).await?;
    let transaction_list: Vec<Vec<String>> = transactions
        .into_iter()
        .map(|transaction| vec![transaction.tx, transaction.instruction])
        .collect();

    Ok(transaction_list)
}

pub async fn get_signatures_for_asset(
    conn: &impl ConnectionTrait,
    asset_id: Option<Vec<u8>>,
    tree_id: Option<Vec<u8>>,
    leaf_idx: Option<i64>,
    pagination: &Pagination,
    limit: u64,
) -> Result<Vec<Vec<String>>, DbErr> {
    // if tree_id and leaf_idx are provided, use them directly to fetch transactions
    if let (Some(tree_id), Some(leaf_idx)) = (tree_id, leaf_idx) {
        let transactions = fetch_transactions(conn, tree_id, leaf_idx, pagination, limit).await?;
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
        let transactions = fetch_transactions(conn, tree, asset.nonce, pagination, limit).await?;
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
    enable_grand_total_query: bool,
) -> Result<(Vec<FullAsset>, Option<u64>), DbErr> {
    if enable_grand_total_query {
        let grand_total_task = get_grand_total(conn, stmt.clone());
        let assets_task = paginate(pagination, limit, stmt).all(conn);

        let (assets, grand_total) = try_join!(assets_task, grand_total_task)?;
        let full_assets = get_related_for_assets(conn, assets).await?;
        return Ok((full_assets, grand_total));
    } else {
        let assets = paginate(pagination, limit, stmt).all(conn).await?;
        let full_assets = get_related_for_assets(conn, assets).await?;
        Ok((full_assets, None))
    }
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
    mut asset_list: AssetList,
) -> Result<AssetList, DbErr> {
    // compile a set of all the distinct group values (bs58 String) from the asset list
    let mut group_values: HashSet<String> = HashSet::new();
    for item in &asset_list.items {
        if let Some(groups) = &item.grouping {
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

    // make a query to fetch all the metadata
    let asset_data = asset_data::Entity::find()
        .filter(asset_data::Column::Id.is_in(bytea_group_values))
        .limit(group_values.len() as u64)
        .all(conn)
        .await?;

    // create a mapping of id -> collection_metadata
    let mut hashmap: HashMap<String, CollectionMetadata> = HashMap::new();
    for data in &asset_data {
        let id = bs58::encode(&data.id).into_string();
        let collection_metadata = get_collection_metadata(&data);
        hashmap.insert(id, collection_metadata);
    }

    // add the metadata to the asset_list
    for item in &mut asset_list.items {
        if let Some(groups) = &mut item.grouping {
            for group in groups {
                if let Some(group_value) = &group.group_value {
                    let collection_metadata = hashmap.get(group_value);
                    if let Some(collection_metadata) = collection_metadata {
                        group.group_key = group.group_key.clone();
                        group.group_value = Some(group_value.clone());
                        group.collection_metadata = Some(collection_metadata.clone());
                    }
                }
            }
        }
    }

    Ok(asset_list)
}

fn get_collection_metadata(data: &asset_data::Model) -> CollectionMetadata {
    let mut chain_data_selector_fn = jsonpath_lib::selector(&data.chain_data);
    let chain_data_selector = &mut chain_data_selector_fn;

    let name = safe_select(chain_data_selector, "$.name");
    let symbol = safe_select(chain_data_selector, "$.symbol");

    let mut metadata_name = "".to_string();
    let mut metadata_symbol = "".to_string();

    if let Some(name) = name {
        metadata_name = name.to_owned().to_string().trim_matches('"').to_string();
    }
    if let Some(symbol) = symbol {
        metadata_symbol = symbol.to_owned().to_string().trim_matches('"').to_string();
    }

    CollectionMetadata {
        name: Some(metadata_name),
        symbol: Some(metadata_symbol),
    }
}
