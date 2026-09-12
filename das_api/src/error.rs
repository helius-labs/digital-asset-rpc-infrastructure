use log::{debug, error, info};
use sea_orm::DbErr;

use {
    jsonrpsee::core::Error as RpcError,
    jsonrpsee::types::error::{CallError, ErrorObject},
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum DasApiError {
    #[error("Config Missing or Error: {0}")]
    ConfigurationError(String),
    #[error("Server Failed to Start")]
    ServerStartError(#[from] RpcError),
    #[error("Database Connection Failed")]
    DatabaseConnectionError(#[from] sqlx::Error),
    #[error("Pubkey Validation Err: {0} is invalid")]
    PubkeyValidationError(String),
    #[error("Validation Error: {0}")]
    ValidationError(String),
    #[error("Database Error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),
    #[error("Pagination Error. Only one pagination parameter supported per query.")]
    PaginationError,
    #[error("Pagination Error. No Pagination Method Selected.")]
    PaginationEmptyError,
    #[error("Deserialization error: {0}")]
    DeserializationError(#[from] serde_json::Error),
    #[error("Paginating beyond 500000 items is not supported. Please use cursor based pagination instead. See https://docs.helius.dev/compression-and-das-api/digital-asset-standard-das-api/pagination")]
    OffsetLimitExceededError,
    #[error("Pagination Error. Limit should not be greater than 1000.")]
    PaginationExceededError,
    #[error("Batch Size Error. Batch size should not be greater than 1000.")]
    BatchSizeExceededError,
    #[error("Pagination Sorting Error. Only sorting based on id is supported for this pagination option.")]
    PaginationSortingValidationError,
    #[error("Cursor Validation Err: {0} is invalid")]
    CursorValidationError(String),
    #[error("Internal Error: {0}")]
    InternalError(String),
}

const INTERNAL_ERROR_CODE: i32 = -32603;
const INVALID_PARAMS_CODE: i32 = -32602;
const RECORD_NOT_FOUND_CODE: i32 = -32000;

impl DasApiError {
    fn log(&self) {
        match self {
            Self::ValidationError(_) => {
                debug!("{}", self);
            }
            Self::DatabaseError(e) => match e {
                DbErr::RecordNotFound(_) => {
                    debug!("{}", e);
                }
                _ => {
                    error!("{}", e);
                }
            },
            Self::DatabaseConnectionError(_)
            | Self::ConfigurationError(_)
            | Self::DeserializationError(_)
            | Self::ServerStartError(_) => {
                error!("{}", self);
            }
            _ => {
                info!("{}", self);
            }
        }
    }

    fn error_code(&self) -> i32 {
        match self {
            Self::PubkeyValidationError(_)
            | Self::CursorValidationError(_)
            | Self::ValidationError(_)
            | Self::PaginationError
            | Self::PaginationEmptyError
            | Self::OffsetLimitExceededError
            | Self::PaginationExceededError
            | Self::PaginationSortingValidationError
            | Self::BatchSizeExceededError
            | Self::DeserializationError(_) => INVALID_PARAMS_CODE,
            Self::DatabaseError(DbErr::RecordNotFound(_)) => RECORD_NOT_FOUND_CODE,
            Self::DatabaseError(_)
            | Self::DatabaseConnectionError(_)
            | Self::ConfigurationError(_)
            | Self::ServerStartError(_)
            | Self::InternalError(_) => INTERNAL_ERROR_CODE,
        }
    }
}

impl From<DasApiError> for RpcError {
    fn from(err: DasApiError) -> RpcError {
        err.log();
        let code = err.error_code();
        RpcError::Call(CallError::Custom(ErrorObject::owned(
            code,
            err.to_string(),
            None::<()>,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(err: DasApiError) -> (i32, String) {
        match RpcError::from(err) {
            RpcError::Call(CallError::Custom(obj)) => (obj.code(), obj.message().to_string()),
            other => panic!("expected CallError::Custom, got {other:?}"),
        }
    }

    fn code(err: DasApiError) -> i32 {
        response(err).0
    }

    #[test]
    fn client_input_faults_map_to_invalid_params() {
        let deserialization = serde_json::from_str::<u8>("\"x\"").unwrap_err();
        let client_faults = [
            DasApiError::PubkeyValidationError("bad".to_string()),
            DasApiError::CursorValidationError("bad".to_string()),
            DasApiError::ValidationError("bad".to_string()),
            DasApiError::PaginationError,
            DasApiError::PaginationEmptyError,
            DasApiError::OffsetLimitExceededError,
            DasApiError::PaginationExceededError,
            DasApiError::PaginationSortingValidationError,
            DasApiError::BatchSizeExceededError,
            DasApiError::DeserializationError(deserialization),
        ];
        for err in client_faults {
            assert_eq!(code(err), INVALID_PARAMS_CODE);
        }
    }

    #[test]
    fn server_faults_map_to_internal_error() {
        let server_faults = [
            DasApiError::ConfigurationError("boom".to_string()),
            DasApiError::InternalError("boom".to_string()),
            DasApiError::DatabaseConnectionError(sqlx::Error::PoolClosed),
            DasApiError::DatabaseError(DbErr::Custom("boom".to_string())),
        ];
        for err in server_faults {
            assert_eq!(code(err), INTERNAL_ERROR_CODE);
        }
    }

    #[test]
    fn record_not_found_keeps_application_code() {
        let err = DasApiError::DatabaseError(DbErr::RecordNotFound("Asset Not Found".to_string()));
        assert_eq!(code(err), RECORD_NOT_FOUND_CODE);
    }

    #[test]
    fn invalid_params_response_carries_our_message() {
        let (code, message) = response(DasApiError::PubkeyValidationError("0OIl".to_string()));
        assert_eq!(code, INVALID_PARAMS_CODE);
        assert_eq!(message, "Pubkey Validation Err: 0OIl is invalid");
    }
}
