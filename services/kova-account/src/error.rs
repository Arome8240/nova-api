use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AccountError {
    #[error("account already exists for this currency")]
    AccountAlreadyExists,

    #[error("account not found")]
    AccountNotFound,

    #[error("unsupported currency")]
    UnsupportedCurrency,

    #[error("account has non-zero balance")]
    AccountHasBalance { balance: String },

    #[error("pending payments exist for this account")]
    PendingPaymentsExist,

    #[error("account is already closed")]
    AlreadyClosed,

    #[error("invalid status transition: {from} → {to}")]
    InvalidTransition { from: String, to: String },

    #[error("pagination limit must be between 1 and 100")]
    InvalidPaginationLimit,

    #[error("unauthorized")]
    Unauthorized,

    #[error("database error")]
    Database(#[from] sqlx::Error),

    #[error("redis error")]
    Redis(#[from] redis::RedisError),

    #[error("ledger client error: {0}")]
    LedgerClient(String),

    #[error("payments client error: {0}")]
    PaymentsClient(String),

    #[error("event publish error: {0}")]
    EventPublish(String),
}

impl IntoResponse for AccountError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AccountError::AccountAlreadyExists => (
                StatusCode::CONFLICT,
                "ACCOUNT_ALREADY_EXISTS",
                "An account for this currency already exists",
            ),
            AccountError::AccountNotFound => {
                (StatusCode::NOT_FOUND, "ACCOUNT_NOT_FOUND", "Account not found")
            }
            AccountError::UnsupportedCurrency => (
                StatusCode::BAD_REQUEST,
                "UNSUPPORTED_CURRENCY",
                "Currency not supported — must be one of: NGN, GBP, USD, KES",
            ),
            AccountError::AccountHasBalance { .. } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "ACCOUNT_HAS_BALANCE",
                "Account must have a zero balance before closure",
            ),
            AccountError::PendingPaymentsExist => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "PENDING_PAYMENTS_EXIST",
                "Account has pending payments — wait for settlement before closure",
            ),
            AccountError::AlreadyClosed => {
                (StatusCode::CONFLICT, "ACCOUNT_ALREADY_CLOSED", "Account is already closed")
            }
            AccountError::InvalidTransition { .. } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_TRANSITION",
                "Invalid account status transition",
            ),
            AccountError::InvalidPaginationLimit => (
                StatusCode::BAD_REQUEST,
                "INVALID_LIMIT",
                "limit must be between 1 and 100",
            ),
            AccountError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Missing or invalid access token")
            }
            AccountError::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                "A database error occurred",
            ),
            AccountError::Redis(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "CACHE_ERROR",
                "A cache error occurred",
            ),
            AccountError::LedgerClient(_) => (
                StatusCode::BAD_GATEWAY,
                "LEDGER_UNAVAILABLE",
                "The ledger service is unavailable",
            ),
            AccountError::PaymentsClient(_) => (
                StatusCode::BAD_GATEWAY,
                "PAYMENTS_UNAVAILABLE",
                "The payments service is unavailable",
            ),
            AccountError::EventPublish(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "EVENT_PUBLISH_ERROR",
                "Failed to publish account event",
            ),
        };

        let detail = match &self {
            AccountError::AccountHasBalance { balance } => {
                Some(json!({ "current_balance": balance }))
            }
            AccountError::InvalidTransition { from, to } => {
                Some(json!({ "from": from, "to": to }))
            }
            _ => None,
        };

        let body = json!({
            "data": null,
            "error": {
                "code": code,
                "message": message,
                "detail": detail
            },
            "meta": {}
        });

        (status, Json(body)).into_response()
    }
}
