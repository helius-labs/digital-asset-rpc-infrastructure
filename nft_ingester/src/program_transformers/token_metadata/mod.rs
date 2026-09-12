mod master_edition;
mod v1_asset;

use crate::{
    config::IngesterConfig,
    error::IngesterError,
    program_transformers::token_metadata::{
        master_edition::{save_edition, save_printable_edition},
        v1_asset::{burn_v1_asset, save_v1_asset},
    },
    tasks::TaskData,
};
use blockbuster::programs::token_metadata::{TokenMetadataAccountData, TokenMetadataAccountState};
use digital_asset_types::dao::sea_orm_active_enums::EditionAccountType;
use plerkle_serialization::AccountInfo;
use sea_orm::DatabaseConnection;
use tokio::sync::mpsc::UnboundedSender;

use self::master_edition::save_v1_edition;

pub async fn handle_token_metadata_account<'a, 'b, 'c>(
    config: &'c IngesterConfig,
    account_update: &'a AccountInfo<'a>,
    parsing_result: &'b TokenMetadataAccountState,
    db: &'c DatabaseConnection,
    task_manager: &UnboundedSender<TaskData>,
) -> Result<(), IngesterError> {
    let key = *account_update.pubkey().unwrap();
    match &parsing_result.data {
        TokenMetadataAccountData::EmptyAccount => {
            burn_v1_asset(db, key, account_update.slot()).await?;
            Ok(())
        }
        TokenMetadataAccountData::MasterEditionV1(m) => {
            save_v1_edition(key, account_update.slot(), m, db).await?;
            Ok(())
        }
        TokenMetadataAccountData::MetadataV1(m) => {
            let task = save_v1_asset(config, db, m, key, account_update.slot()).await?;
            if !config.skip_offchain.unwrap_or(false) {
                if let Some(task) = task {
                    task_manager.send(task)?;
                }
            }
            Ok(())
        }
        TokenMetadataAccountData::MasterEditionV2(m) => {
            save_edition(
                key,
                EditionAccountType::MasterEditionV2,
                account_update.slot(),
                m,
                db,
            )
            .await?;
            Ok(())
        }
        TokenMetadataAccountData::EditionV1(e) => {
            save_printable_edition(
                key,
                EditionAccountType::Edition,
                account_update.slot(),
                e,
                db,
            )
            .await?;
            Ok(())
        }
        // TokenMetadataAccountData::EditionMarker(_) => {}
        // TokenMetadataAccountData::UseAuthorityRecord(_) => {}
        // TokenMetadataAccountData::CollectionAuthorityRecord(_) => {}
        _ => Err(IngesterError::NotImplemented),
    }?;
    Ok(())
}
