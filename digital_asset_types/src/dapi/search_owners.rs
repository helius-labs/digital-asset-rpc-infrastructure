use super::common::{build_owner_response, create_pagination};
use crate::{
    dao::{scopes, PageOptions}, dapi::last_indexed_slot::load_last_indexed_slot, rpc::{options::Options, response::OwnerList}
};
use sea_orm::{DatabaseConnection, DbErr};

pub async fn search_owners(
    db: &DatabaseConnection,
    asset: Vec<u8>,
    page_options: &PageOptions,
    options: &Options,
) -> Result<OwnerList, DbErr> {
    let last_indexed_slot = load_last_indexed_slot(db).await?;
    let pagination = create_pagination(&page_options)?;
    let owners =
        scopes::asset::get_owners(db, asset, &pagination, page_options.limit, options).await?;
    let owner_list = build_owner_response(last_indexed_slot, owners, page_options.limit, &pagination);
    Ok(owner_list)
}
