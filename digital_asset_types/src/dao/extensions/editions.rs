use sea_orm::{EntityTrait, EnumIter, Related, RelationDef, RelationTrait};

use crate::dao::{asset, editions};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    AssetEdition,
    AssetParentEdition,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::AssetEdition => editions::Entity::belongs_to(asset::Entity)
                .from(editions::Column::Id)
                .to(asset::Column::EditionAddress)
                .into(),
            Self::AssetParentEdition => editions::Entity::belongs_to(asset::Entity)
                .from(editions::Column::Parent)
                .to(asset::Column::EditionAddress)
                .into(),
        }
    }
}

impl Related<asset::Entity> for editions::Entity {
    fn to() -> RelationDef {
        Relation::AssetEdition.def()
    }
}
