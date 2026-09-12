use super::common::{build_token_account_response, create_pagination};
use crate::{
    dao::{scopes, PageOptions},
    dapi::{common::create_token_sorting, last_indexed_slot::load_last_indexed_slot},
    rpc::{filter::TokenSorting, options::Options, response::TokenAccountsList},
};
use sea_orm::{DatabaseConnection, DbErr};

pub async fn get_token_accounts(
    db: &DatabaseConnection,
    owner: Option<Vec<u8>>,
    mint: Option<Vec<u8>>,
    sort_by: TokenSorting,
    page_options: &PageOptions,
    options: &Options,
) -> Result<TokenAccountsList, DbErr> {
    let last_indexed_slot = load_last_indexed_slot(db).await?;
    let pagination = create_pagination(page_options)?;
    let (sort_direction, _sort_column) = create_token_sorting(sort_by);
    let token_accounts = scopes::asset::get_token_accounts(
        db,
        owner,
        mint,
        sort_direction,
        &pagination,
        page_options.limit,
        options,
    )
    .await?;
    let token_accounts_list =
        build_token_account_response(last_indexed_slot, token_accounts, page_options.limit, &pagination);

    Ok(token_accounts_list)
}
