//! SeaORM Entity. Generated by sea-orm-codegen 0.9.2

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Default, Debug, DeriveEntity)]
pub struct Entity;

impl EntityName for Entity {
    fn table_name(&self) -> &str {
        "token_accounts"
    }
}

#[derive(Clone, Debug, PartialEq, DeriveModel, DeriveActiveModel, Serialize, Deserialize)]
pub struct Model {
    pub pubkey: Vec<u8>,
    pub mint: Option<Vec<u8>>,
    pub amount: i64,
    pub owner: Vec<u8>,
    pub frozen: bool,
    pub close_authority: Option<Vec<u8>>,
    pub delegate: Option<Vec<u8>>,
    pub delegated_amount: i64,
    pub slot_updated: i64,
    pub token_program: Vec<u8>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveColumn)]
pub enum Column {
    Pubkey,
    Mint,
    Amount,
    Owner,
    Frozen,
    CloseAuthority,
    Delegate,
    DelegatedAmount,
    SlotUpdated,
    TokenProgram,
}

#[derive(Copy, Clone, Debug, EnumIter, DerivePrimaryKey)]
pub enum PrimaryKey {
    Pubkey,
}

impl PrimaryKeyTrait for PrimaryKey {
    type ValueType = Vec<u8>;
    fn auto_increment() -> bool {
        false
    }
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Tokens,
}

impl ColumnTrait for Column {
    type EntityName = Entity;
    fn def(&self) -> ColumnDef {
        match self {
            Self::Pubkey => ColumnType::Binary.def(),
            Self::Mint => ColumnType::Binary.def().null(),
            Self::Amount => ColumnType::BigInteger.def(),
            Self::Owner => ColumnType::Binary.def(),
            Self::Frozen => ColumnType::Boolean.def(),
            Self::CloseAuthority => ColumnType::Binary.def().null(),
            Self::Delegate => ColumnType::Binary.def().null(),
            Self::DelegatedAmount => ColumnType::BigInteger.def(),
            Self::SlotUpdated => ColumnType::BigInteger.def(),
            Self::TokenProgram => ColumnType::Binary.def(),
        }
    }
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Tokens => Entity::belongs_to(super::tokens::Entity)
                .from(Column::Mint)
                .to(super::tokens::Column::Mint)
                .into(),
        }
    }
}

impl Related<super::tokens::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tokens.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
