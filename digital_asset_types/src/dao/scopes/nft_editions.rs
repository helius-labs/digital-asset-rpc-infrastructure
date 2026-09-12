use crate::{
    dao::{
        asset, editions, extensions, sea_orm_active_enums::EditionAccountType, EditionInfo,
        Pagination,
    },
    rpc::response::{Edition, EditionsList},
};
use sea_orm::{
    sea_query::{Alias, Expr},
    ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, FromQueryResult, JoinType, Order,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, FromQueryResult)]
pub struct EditionData {
    pub id: Vec<u8>,
    pub mint: Vec<u8>,
    pub parent: Option<Vec<u8>>,
    pub data: Option<Json>,
    pub edition_type: EditionAccountType,
}

pub async fn get_nft_editions(
    conn: &impl ConnectionTrait,
    last_indexed_slot: u64,
    mint: Option<Vec<u8>>,
    pagination: &Pagination,
    limit: u64,
) -> Result<EditionsList, DbErr> {
    fn get_page_number(pagination: &Pagination) -> u64 {
        match pagination {
            Pagination::Page { page } => *page,
            _ => 1,
        }
    }
    let page_number = get_page_number(pagination);
    let edition_relation = extensions::editions::Relation::AssetParentEdition
        .def()
        .rev();

    let master_relation = extensions::editions::Relation::AssetEdition.def().rev();
    let asset_relation = extensions::asset::Relation::MasterEdition.def().rev();

    let master_edition = asset::Entity::find()
        .join(JoinType::LeftJoin, master_relation)
        .select_only()
        .column_as(asset::Column::Id, "mint")
        .column(editions::Column::Id)
        .column(editions::Column::Data)
        .column(editions::Column::Parent)
        .column(editions::Column::EditionType)
        .filter(asset::Column::Id.eq(mint.clone()))
        .into_model::<EditionData>()
        .one(conn)
        .await?;

    let mut editions_list = EditionsList::default();
    editions_list.last_indexed_slot = last_indexed_slot;
    if let Some(edition_data) = master_edition {
        if edition_data.edition_type == EditionAccountType::MasterEditionV1
            || edition_data.edition_type == EditionAccountType::MasterEditionV2
        {
            editions_list.master_edition_address = bs58::encode(edition_data.id).into_string();
            editions_list.supply = edition_data
                .data
                .as_ref()
                .and_then(|d| d.get("supply").and_then(|v| v.as_u64()))
                .unwrap_or_default();
            editions_list.max_supply = edition_data
                .data
                .as_ref()
                .and_then(|d| d.get("max_supply"))
                .and_then(|v| v.as_u64());
        } else {
            return Err(DbErr::RecordNotFound(
                "Master Edition Not Found".to_string(),
            ));
        }
    } else {
        return Err(DbErr::RecordNotFound("Asset Not Found".to_string()));
    }

    let stmt = asset::Entity::find()
        .join(JoinType::LeftJoin, edition_relation)
        .join_as(
            JoinType::LeftJoin,
            asset_relation,
            Alias::new("asset_editions"),
        )
        .select_only()
        .column(asset::Column::EditionAddress)
        .column_as(Expr::cust("asset_editions.id"), "mint")
        .column(editions::Column::Id)
        .column(editions::Column::Data)
        .column(editions::Column::Parent)
        .column(editions::Column::EditionType)
        .filter(asset::Column::Id.eq(mint.clone()))
        .limit(limit)
        .offset((page_number - 1) * limit)
        .order_by(
            Expr::cust("(editions.data->>'edition')::BIGINT"),
            Order::Asc,
        )
        .into_model::<EditionData>()
        .all(conn)
        .await?;

    let mut editions = Vec::new();

    for edition_data in stmt {
        if edition_data.edition_type == EditionAccountType::Edition {
            let edition_number = edition_data
                .data
                .and_then(|d| d.get("edition").and_then(|v| v.as_u64()));
            editions.push(Edition {
                mint: bs58::encode(edition_data.mint).into_string(),
                edition_address: bs58::encode(edition_data.id).into_string(),
                edition: edition_number,
            });
        }
    }

    editions_list.total = editions.len() as u32;
    editions_list.limit = limit as u32;
    editions_list.page = Some(page_number as u32);
    editions_list.editions = editions;
    Ok(editions_list)
}

pub async fn get_related_edition(
    conn: &impl ConnectionTrait,
    edition_address: Vec<u8>,
) -> Result<Option<EditionInfo>, DbErr> {
    let condition = Condition::all().add(editions::Column::Id.eq(edition_address.clone()));

    let edition_result = editions::Entity::find().filter(condition).one(conn).await;

    match edition_result {
        Ok(Some(edition)) => {
            if edition.edition_type == EditionAccountType::Edition {
                let master_cond =
                    Condition::all().add(asset::Column::EditionAddress.eq(edition.parent.clone()));
                let master_relation = extensions::editions::Relation::AssetEdition.def().rev();
                let mut final_result = EditionInfo::from(edition);
                let master_edition = asset::Entity::find()
                    .join(JoinType::LeftJoin, master_relation)
                    .select_only()
                    .column_as(asset::Column::Id, "mint")
                    .column(editions::Column::Id)
                    .column(editions::Column::Data)
                    .column(editions::Column::Parent)
                    .column(editions::Column::EditionType)
                    .filter(master_cond)
                    .into_model::<EditionData>()
                    .one(conn)
                    .await?;

                if let Some(edition_data) = master_edition {
                    if edition_data.edition_type == EditionAccountType::MasterEditionV1
                        || edition_data.edition_type == EditionAccountType::MasterEditionV2
                    {
                        final_result.master_edition_mint =
                            Some(bs58::encode(edition_data.mint).into_string());
                        final_result.supply = edition_data
                            .data
                            .as_ref()
                            .and_then(|d| d.get("supply").and_then(|v| v.as_u64()));
                        final_result.max_supply = edition_data
                            .data
                            .as_ref()
                            .and_then(|d| d.get("max_supply"))
                            .and_then(|v| v.as_u64());
                    }
                }
                Ok(Some(final_result))
            } else {
                Ok(Some(EditionInfo::from(edition)))
            }
        }
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

pub async fn get_related_editions(
    conn: &impl ConnectionTrait,
    edition_address: Vec<Vec<u8>>,
) -> Result<Vec<EditionInfo>, DbErr> {
    let condition = Condition::all().add(editions::Column::Id.is_in(edition_address));

    let editions_result = editions::Entity::find().filter(condition).all(conn).await;

    match editions_result {
        Ok(editions) => {
            let parent_addresses = editions
                .iter()
                .filter_map(|edition| {
                    if edition.edition_type == EditionAccountType::Edition {
                        Some(edition.parent.clone())
                    } else {
                        None
                    }
                })
                .flatten()
                .collect::<Vec<Vec<u8>>>();

            let mut parent_editions_map = std::collections::HashMap::new();

            if !parent_addresses.is_empty() {
                let parent_condition =
                    Condition::all().add(editions::Column::Id.is_in(parent_addresses));
                let master_relation = extensions::editions::Relation::AssetEdition.def().rev();
                let parent_editions_result = asset::Entity::find()
                    .join(JoinType::LeftJoin, master_relation)
                    .select_only()
                    .column_as(asset::Column::Id, "mint")
                    .column(editions::Column::Id)
                    .column(editions::Column::Data)
                    .column(editions::Column::Parent)
                    .column(editions::Column::EditionType)
                    .filter(parent_condition)
                    .into_model::<EditionData>()
                    .all(conn)
                    .await?;

                for parent_edition in parent_editions_result {
                    let key = bs58::encode(parent_edition.id.clone()).into_string();
                    parent_editions_map.insert(key, parent_edition);
                }
            }
            let edition_infos = editions
                .into_iter()
                .map(|edition| {
                    let mut final_edition_info = EditionInfo::from(edition);
                    if let Some(parent_address) = final_edition_info.parent.as_ref() {
                        if let Some(parent_info) = parent_editions_map.get(parent_address) {
                            final_edition_info.master_edition_mint =
                                Some(bs58::encode(parent_info.mint.clone()).into_string());
                            final_edition_info.supply = parent_info
                                .data
                                .as_ref()
                                .and_then(|d| d.get("supply").and_then(|v| v.as_u64()));
                            final_edition_info.max_supply = parent_info
                                .data
                                .as_ref()
                                .and_then(|d| d.get("max_supply").and_then(|v| v.as_u64()));
                        }
                    }
                    final_edition_info
                })
                .collect();

            Ok(edition_infos)
        }
        Err(e) => Err(e),
    }
}

impl From<editions::Model> for EditionInfo {
    fn from(edition: editions::Model) -> Self {
        let edition_type_str = edition.edition_type.to_string();
        let address = bs58::encode(&edition.id).into_string();

        match edition.edition_type {
            EditionAccountType::MasterEditionV1 | EditionAccountType::MasterEditionV2 => {
                let supply = edition
                    .data
                    .clone()
                    .and_then(|data| data.get("supply").and_then(|v| v.as_u64()));
                let max_supply = edition
                    .data
                    .and_then(|data| data.get("max_supply").and_then(|v| v.as_u64()));

                Self {
                    address,
                    edition_type: edition_type_str,
                    supply,
                    max_supply,
                    ..Default::default()
                }
            }
            EditionAccountType::Edition => {
                let edition_number = edition
                    .data
                    .and_then(|data| data.get("edition").and_then(|v| v.as_u64()));
                let parent = edition.parent.map(|p| bs58::encode(&p).into_string());

                Self {
                    address,
                    edition_type: edition_type_str,
                    edition: edition_number,
                    parent,
                    ..Default::default()
                }
            }
            _ => Self::default(),
        }
    }
}

impl From<&str> for EditionAccountType {
    fn from(edition_type: &str) -> Self {
        match edition_type.trim_matches('\'') {
            "edition" => EditionAccountType::Edition,
            "edition_marker" => EditionAccountType::EditionMarker,
            "master_edition_v1" => EditionAccountType::MasterEditionV1,
            "master_edition_v2" => EditionAccountType::MasterEditionV2,
            _ => EditionAccountType::Unknown,
        }
    }
}
