use sea_orm::{EntityTrait, EnumIter, Related, RelationDef, RelationTrait};

use crate::dao::{asset, price, tokens};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Asset,
    Tokens,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Asset => price::Entity::has_many(asset::Entity).into(),
            Self::Tokens => price::Entity::has_many(tokens::Entity).into(),
        }
    }
}

impl Related<asset::Entity> for price::Entity {
    fn to() -> RelationDef {
        Relation::Asset.def()
    }
}

impl Related<tokens::Entity> for price::Entity {
    fn to() -> RelationDef {
        Relation::Tokens.def()
    }
}
