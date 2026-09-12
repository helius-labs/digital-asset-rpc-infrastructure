use {
    self::v1_asset::{burn_v1_asset, save_v1_asset},
    crate::{config::IngesterConfig, error::IngesterError, tasks::TaskData},
    blockbuster::programs::mpl_core_program::{MplCoreAccountData, MplCoreAccountState},
    plerkle_serialization::AccountInfo,
    sea_orm::DatabaseConnection,
    tokio::sync::mpsc::UnboundedSender,
};

mod v1_asset;

pub async fn handle_mpl_core_account<'a, 'b, 'c>(
    config: &'c IngesterConfig,
    account_update: &'a AccountInfo<'a>,
    parsing_result: &'b MplCoreAccountState,
    db: &'c DatabaseConnection,
    task_manager: &UnboundedSender<TaskData>,
) -> Result<(), IngesterError> {
    let key = *account_update.pubkey().unwrap();
    match &parsing_result.data {
        MplCoreAccountData::EmptyAccount => {
            burn_v1_asset(db, key, account_update.slot()).await?;
            Ok(())
        }
        MplCoreAccountData::Asset(_)
        | MplCoreAccountData::Collection(_)
        | MplCoreAccountData::Group { .. } => {
            let task = save_v1_asset(
                account_update,
                db,
                key,
                &parsing_result.data,
                account_update.slot(),
            )
            .await?;
            if !config.skip_offchain.unwrap_or(false) {
                if let Some(task) = task {
                    task_manager.send(task)?;
                }
            }
            Ok(())
        }
        _ => Err(IngesterError::NotImplemented),
    }?;
    Ok(())
}
