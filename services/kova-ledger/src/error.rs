use tonic::{Code, Status};

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// idempotency_key already exists with a different payload hash
    #[error("idempotency key exists with a different payload")]
    IdempotencyMismatch,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("decimal parse error: {0}")]
    DecimalParse(String),

    #[error("internal: {0}")]
    Internal(String),
}

impl From<rust_decimal::Error> for LedgerError {
    fn from(e: rust_decimal::Error) -> Self {
        LedgerError::DecimalParse(e.to_string())
    }
}

impl From<LedgerError> for Status {
    fn from(e: LedgerError) -> Self {
        match e {
            LedgerError::InvalidArgument(msg) => Status::new(Code::InvalidArgument, msg),
            LedgerError::IdempotencyMismatch => Status::new(
                Code::AlreadyExists,
                "idempotency key exists with a different payload",
            ),
            LedgerError::DecimalParse(msg) => {
                Status::new(Code::InvalidArgument, format!("invalid decimal: {msg}"))
            }
            LedgerError::Database(ref e) => {
                tracing::error!(error = %e, "database error in gRPC handler");
                Status::internal("internal database error")
            }
            LedgerError::Internal(ref msg) => {
                tracing::error!("internal ledger error: {msg}");
                Status::internal(msg.clone())
            }
        }
    }
}
