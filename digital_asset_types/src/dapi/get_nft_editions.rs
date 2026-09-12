use super::common::create_pagination;
use crate::{
    dao::{scopes, PageOptions}, dapi::last_indexed_slot::load_last_indexed_slot, rpc::response::EditionsList
};
use sea_orm::{DatabaseConnection, DbErr};

pub async fn get_nft_editions(
    db: &DatabaseConnection,
    mint: Option<Vec<u8>>,
    page_options: &PageOptions,
) -> Result<EditionsList, DbErr> {
    let last_indexed_slot = load_last_indexed_slot(db).await?;
    let pagination = create_pagination(page_options)?;
    let editions =
        scopes::nft_editions::get_nft_editions(db, last_indexed_slot, mint, &pagination, page_options.limit).await?;

    Ok(editions)
}
