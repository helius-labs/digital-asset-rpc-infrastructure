use sea_orm::DbErr;
use solana_sdk::pubkey::ParsePubkeyError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum DasJobErr {
    #[error("Db Error: {0}")]
    DbError(String),
    #[error("Parse Pubkey Error: {0}")]
    ParsePubkeyError(String),
    #[error("Configuration Error: {0}")]
    ConfigurationError(String),
    #[error("Account Forward Error: {0}")]
    AccountForwardError(String),
    #[error("Reindex Error: {0}")]
    ReindexError(String),
    #[error("AssetData Migration Error: {0}")]
    AssetDataMigrationError(String),
    #[error("Db Error: {0}")]
    RpcError(String),
}

impl From<DbErr> for DasJobErr {
    fn from(e: DbErr) -> Self {
        DasJobErr::DbError(e.to_string())
    }
}
impl From<ParsePubkeyError> for DasJobErr {
    fn from(e: ParsePubkeyError) -> Self {
        DasJobErr::DbError(e.to_string())
    }
}
