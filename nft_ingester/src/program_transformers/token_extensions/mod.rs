mod metadata_account;
mod mint;
mod token_account;

use crate::{config::IngesterConfig, error::IngesterError, tasks::TaskData};
use blockbuster::programs::token_extensions::TokenExtensionsProgramAccount;
use plerkle_serialization::AccountInfo;
use sea_orm::DatabaseConnection;
use tokio::sync::mpsc::UnboundedSender;

use self::{
    metadata_account::handle_token_metadata_account, mint::handle_token_extensions_mint_account,
    token_account::handle_token_extensions_token_account,
};

pub async fn handle_token_extensions_program_account<'a, 'b, 'c>(
    account_update: &'a AccountInfo<'a>,
    parsing_result: &'b TokenExtensionsProgramAccount,
    db: &'c DatabaseConnection,
    task_manager: &UnboundedSender<TaskData>,
    config: &IngesterConfig,
) -> Result<(), IngesterError> {
    match &parsing_result {
        TokenExtensionsProgramAccount::TokenAccount(ta) => {
            handle_token_extensions_token_account(ta, account_update, db).await?;
            Ok(())
        }
        TokenExtensionsProgramAccount::MintAccount(m) => {
            let task = handle_token_extensions_mint_account(m, account_update, db).await?;
            if let Some(t) = task {
                if !config.skip_offchain.unwrap_or(false) {
                    task_manager.send(t)?;
                }
            }
            Ok(())
        }
        TokenExtensionsProgramAccount::TokenMetadataAccount(tma) => {
            handle_token_metadata_account(tma, account_update, db).await?;
            Ok(())
        }
        _ => Err(IngesterError::NotImplemented),
    }?;
    Ok(())
}
