use sea_orm::{EntityTrait, EnumIter, Related, RelationDef, RelationTrait};

use crate::dao::{asset, asset_data_v2};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Asset,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Asset => asset_data_v2::Entity::has_many(asset::Entity).into(),
        }
    }
}

impl Related<asset::Entity> for asset_data_v2::Entity {
    fn to() -> RelationDef {
        Relation::Asset.def()
    }
}
