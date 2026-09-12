mod acc_asset;
pub mod extensions;
mod full_asset;
mod generated;
pub mod scopes;
pub use acc_asset::*;
use chrono::{DateTime, Utc};
pub use full_asset::*;
#[allow(ambiguous_glob_reexports)]
pub use generated::*;
use schemars::JsonSchema;

use self::{
    scopes::asset::TokenType,
    sea_orm_active_enums::{
        OwnerType, RoyaltyTargetType, SpecificationAssetClass, SpecificationVersions,
    },
};
use sea_orm::{
    entity::*,
    sea_query::Expr,
    sea_query::{ConditionType, IntoCondition, SimpleExpr},
    Condition, DbErr, RelationDef,
};
use serde::{Deserialize, Serialize};

pub struct GroupingSize {
    pub size: u64,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct PageOptions {
    pub limit: u64,
    pub page: Option<u64>,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
    pub cursor: Option<Cursor>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Cursor {
    pub id: Option<Vec<u8>>,
}

pub enum Pagination {
    Keyset {
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
    },
    Page {
        page: u64,
    },
    Cursor(Cursor),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NotFilter {
    pub collections: Option<Vec<String>>,
    pub owners: Option<Vec<Vec<u8>>>,
    pub creators: Option<Vec<Vec<u8>>>,
    pub authorities: Option<Vec<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreatedAtFilter {
    #[serde(default)]
    pub after: Option<DateTime<Utc>>,
    #[serde(default)]
    pub before: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchAssetsQuery {
    // Conditions
    pub negate: Option<bool>,
    pub not_filter: Option<NotFilter>,
    /// Defaults to [ConditionType::All]
    pub condition_type: Option<ConditionType>,
    pub specification_version: Option<SpecificationVersions>,
    pub specification_asset_class: Option<SpecificationAssetClass>,
    pub owner_address: Option<Vec<u8>>,
    pub owner_type: Option<OwnerType>,
    pub creator_address: Option<Vec<u8>>,
    pub creator_verified: Option<bool>,
    pub authority_address: Option<Vec<u8>>,
    pub grouping: Option<(String, String)>,
    pub delegate: Option<Vec<u8>>,
    pub frozen: Option<bool>,
    pub supply: Option<u64>,
    pub supply_mint: Option<Vec<u8>>,
    pub compressed: Option<bool>,
    pub compressible: Option<bool>,
    pub royalty_target_type: Option<RoyaltyTargetType>,
    pub royalty_target: Option<Vec<u8>>,
    pub royalty_amount: Option<u32>,
    pub burnt: Option<bool>,
    pub json_uri: Option<String>,
    pub name: Option<Vec<u8>>,
    pub collections: Option<Vec<String>>,
    pub token_type: Option<TokenType>,
    pub created_at: Option<CreatedAtFilter>,
    pub tree: Option<Vec<u8>>,
    pub collection_nft: Option<bool>,
    pub is_agent: Option<bool>,
    pub agent_token: Option<Vec<u8>>,
    pub asset_signer: Option<Vec<u8>>,
}

impl SearchAssetsQuery {
    pub fn get_conditions(&self) -> Condition {
        let mut conditions = match self.condition_type {
            // None --> default to all when no option is provided
            None | Some(ConditionType::All) => Condition::all(),
            Some(ConditionType::Any) => Condition::any(),
        };

        conditions = conditions
            .add_option(
                self.specification_version
                    .clone()
                    .map(|x| asset::Column::SpecificationVersion.eq(x)),
            )
            .add_option(
                self.specification_asset_class
                    .clone()
                    .map(|x| asset::Column::SpecificationAssetClass.eq(x)),
            )
            .add_option(
                self.owner_address
                    .to_owned()
                    .map(|x| asset::Column::Owner.eq(x)),
            )
            .add_option(
                self.delegate
                    .to_owned()
                    .map(|x| asset::Column::Delegate.eq(x)),
            )
            .add_option(self.frozen.map(|x| asset::Column::Frozen.eq(x)))
            .add_option(
                self.supply_mint
                    .to_owned()
                    .map(|x| asset::Column::SupplyMint.eq(x)),
            )
            .add_option(self.compressed.map(|x| asset::Column::Compressed.eq(x)))
            .add_option(self.compressible.map(|x| asset::Column::Compressible.eq(x)))
            .add_option(
                self.royalty_target_type
                    .clone()
                    .map(|x| asset::Column::RoyaltyTargetType.eq(x)),
            )
            .add_option(
                self.royalty_target
                    .to_owned()
                    .map(|x| asset::Column::RoyaltyTarget.eq(x)),
            )
            .add_option(
                self.royalty_amount
                    .map(|x| asset::Column::RoyaltyAmount.eq(x)),
            )
            .add_option(self.burnt.map(|x| asset::Column::Burnt.eq(x)))
            .add_option(self.tree.to_owned().map(|x| asset::Column::TreeId.eq(x)))
            .add_option(self.is_agent.map(|x| asset::Column::IsAgent.eq(x)))
            .add_option(
                self.agent_token
                    .to_owned()
                    .map(|x| asset::Column::AgentToken.eq(x)),
            )
            .add_option(
                self.asset_signer
                    .to_owned()
                    .map(|x| asset::Column::AssetSigner.eq(x)),
            );

        if let Some(s) = self.supply {
            conditions = conditions.add(asset::Column::Supply.eq(s));
        } else {
            // By default, we ignore malformed tokens by ignoring tokens with supply=0
            // unless they are burnt.
            //
            // cNFTs keep supply=1 after they are burnt.
            // Regular NFTs go to supply=0 after they are burnt.
            conditions = conditions.add(
                asset::Column::Supply
                    .ne(0)
                    .or(asset::Column::Burnt.eq(true)),
            )
        }

        // In theory, the owner_type=single check should be sufficient,
        // however there is an old bug that has marked some non-NFTs as "single" with supply > 1.
        // The supply check guarentees we do not include those.
        let nft_condition = asset::Column::OwnerType
            .eq(OwnerType::Single)
            .and(asset::Column::Supply.lte(1));

        // DAS by default is only for NFTs.
        match self.token_type.clone().unwrap_or(TokenType::NonFungible) {
            TokenType::NonFungible => {
                conditions = conditions.add_option(Some(nft_condition));
            }
            TokenType::CompressedNft => {
                conditions = conditions
                    .add_option(Some(nft_condition.and(asset::Column::Compressed.eq(true))))
            }
            TokenType::RegularNft => {
                conditions = conditions
                    .add_option(Some(nft_condition.and(asset::Column::Compressed.eq(false))))
            }
            _ => {}
        }
        if let Some(created_at) = &self.created_at {
            if let Some(after) = created_at.after {
                // Left inclusive
                conditions = conditions.add_option(Some(asset::Column::CreatedAt.gte(after)));
            }
            if let Some(before) = created_at.before {
                // Right exclusive
                conditions = conditions.add_option(Some(asset::Column::CreatedAt.lt(before)));
            }
        }
        conditions
    }

    pub fn conditions(&self) -> Result<(Condition, Vec<RelationDef>), DbErr> {
        let mut conditions = self.get_conditions();

        let mut joins = Vec::new();

        conditions = if let Some(true) = self.negate {
            conditions.add_option(
                self.owner_type
                    .clone()
                    .map(|x| asset::Column::OwnerType.eq(x)),
            )
        } else {
            conditions.add(
                asset::Column::OwnerType.eq(self.owner_type.clone().unwrap_or(OwnerType::Single)),
            )
        };

        // keeps track of what joins need to be added
        let mut asset_creator_join = false;
        let mut asset_data_join = false;

        if let Some(c) = self.creator_address.to_owned() {
            conditions = conditions.add(asset_creators::Column::Creator.eq(c));
        }

        // Without specifying the creators themselves, there is no index being hit.
        // So in some rare scenarios, this query could be very slow.
        if let Some(cv) = self.creator_verified.to_owned() {
            conditions = conditions.add(asset_creators::Column::Verified.eq(cv));
        }

        // If creator_address or creator_verified is set, join with asset_creators
        if self.creator_address.is_some() || self.creator_verified.is_some() {
            asset_creator_join = true;
        }

        if let Some(a) = self.authority_address.to_owned() {
            let authority_expr = SimpleExpr::Custom(format!(
                "authorities_info -> 'authority' = '{}'::jsonb",
                serde_json::to_string(&a).unwrap_or("[]".to_string())
            ));
            conditions = conditions.add(authority_expr);
        }

        if let Some(g) = self.grouping.to_owned() {
            let collection_expr =
                SimpleExpr::Custom(format!("collections_info ->> 'collection_id' = '{}'", g.1));
            conditions = conditions.add(collection_expr);
        }

        if let Some(n) = self.collection_nft.to_owned() {
            if n {
                let collection_expr = SimpleExpr::Custom(
                    "collections_info ->> 'collection_nft' = 'true'".to_string(),
                );
                conditions = conditions.add(collection_expr);
            } else {
                let collection_expr = SimpleExpr::Custom(
                    "(collections_info -> 'collection_nft' = 'null' OR collections_info -> 'collection_nft' IS NULL OR collections_info ->> 'collection_nft' = 'false')".to_string(),
                );

                conditions = conditions.add(collection_expr);
            }
        }

        if let Some(c) = self.collections.to_owned() {
            let mut combined_exists = "collections_info ->> 'collection_id' = ''".to_owned();
            if !c.is_empty() {
                let exists_conds: Vec<String> = c
                    .iter()
                    .map(|collection| {
                        format!("collections_info ->> 'collection_id' = '{}'", collection)
                    })
                    .collect();
                combined_exists = exists_conds.join(" OR ");
            }
            let cond = SimpleExpr::Custom(format!("({})", combined_exists));
            conditions = conditions.add(cond);
        }

        if let Some(ju) = self.json_uri.to_owned() {
            let cond = Condition::all().add(asset_data_v2::Column::MetadataUrl.eq(ju));
            conditions = conditions.add(cond);
            asset_data_join = true;
        }

        if let Some(not_filter) = self.not_filter.to_owned() {
            if let Some(owners) = not_filter.owners {
                let cond = Condition::all().add(asset::Column::Owner.is_not_in(owners));
                conditions = conditions.add(cond);
            }

            if let Some(creators) = not_filter.creators {
                let cond =
                    Condition::all().add(asset_creators::Column::Creator.is_not_in(creators));
                conditions = conditions.add(cond);
                asset_creator_join = true;
            }

            if let Some(authorities) = not_filter.authorities {
                let not_exists_conds: Vec<String> = authorities
                    .iter()
                    .map(|authority| {
                        format!(
                            "NOT (authorities_info -> 'authority' = '{}'::jsonb)",
                            serde_json::to_string(&authority).unwrap()
                        )
                    })
                    .collect();
                let combined_not_exists = not_exists_conds.join(" AND ");
                let cond = SimpleExpr::Custom(combined_not_exists);
                conditions = conditions.add(cond);
            }

            if let Some(groups) = not_filter.collections {
                let not_exists_conds: Vec<String> = groups
                    .iter()
                    .map(|group| format!("NOT collections_info ->> 'collection_id' = '{}'", group))
                    .collect();
                let combined_not_exists = not_exists_conds.join(" AND ");
                let cond = SimpleExpr::Custom(combined_not_exists);
                conditions = conditions.add(cond);
            }
        }

        if asset_creator_join {
            let rel = extensions::asset_creators::Relation::Asset
                .def()
                .rev()
                .on_condition(|left, right| {
                    Expr::tbl(right, asset_creators::Column::AssetId)
                        .eq(Expr::tbl(left, asset::Column::Id))
                        .into_condition()
                });
            joins.push(rel);
        }
        if asset_data_join {
            let rel = extensions::asset_data_v2::Relation::Asset
                .def()
                .rev()
                .on_condition(|left, right| {
                    Expr::tbl(right, asset_data_v2::Column::Id)
                        .eq(Expr::tbl(left, asset::Column::Id))
                        .into_condition()
                });
            joins.push(rel);
        }

        if let Some(n) = self.name.to_owned() {
            let name_as_str = std::str::from_utf8(&n).map_err(|_| {
                DbErr::Custom(
                    "Could not convert raw name bytes into string for comparison".to_owned(),
                )
            })?;

            let name_expr =
                SimpleExpr::Custom(format!("chain_data->>'name' LIKE '%{}%'", name_as_str));
            conditions = conditions.add(name_expr);
            let rel = extensions::asset_data_v2::Relation::Asset
                .def()
                .rev()
                .on_condition(|left, right| {
                    Expr::tbl(right, asset_data_v2::Column::Id)
                        .eq(Expr::tbl(left, asset::Column::Id))
                        .into_condition()
                });
            joins.push(rel);
        }

        Ok((
            match self.negate {
                None | Some(false) => conditions,
                Some(true) => conditions.not(),
            },
            joins,
        ))
    }
}
