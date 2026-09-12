use sea_orm::{EntityTrait, EnumIter, Related, RelationDef, RelationTrait};

use crate::dao::{price, tokens};

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Price,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Price => tokens::Entity::belongs_to(price::Entity)
                .from(tokens::Column::Mint)
                .to(price::Column::Mint)
                .into(),
        }
    }
}

impl Related<price::Entity> for tokens::Entity {
    fn to() -> RelationDef {
        Relation::Price.def()
    }
}
