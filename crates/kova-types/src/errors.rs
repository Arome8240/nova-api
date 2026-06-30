//! Shared error hierarchy for all KOVA services.
//!
//! Library crates (this crate, all service modules) must use these types —
//! never `anyhow`. `anyhow` is acceptable only in binary crate `main.rs`.

use serde::Serialize;
use thiserror::Error;

// ── Leaf error types ──────────────────────────────────────────────────────────

/// Wraps a database error without surfacing SQL details in user-facing messages.
#[derive(Debug, Error, Serialize)]
#[error("a database error occurred")]
pub struct DatabaseError {
    /// Human-readable context (safe to log internally).
    pub message: String,
    /// The underlying sqlx error — skipped in JSON serialisation.
    #[serde(skip)]
    #[source]
    pub source: Option<sqlx::Error>,
}

impl DatabaseError {
    pub fn new(err: sqlx::Error) -> Self {
        Self {
            message: "a database error occurred".to_string(),
            source: Some(err),
        }
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }
}

impl From<sqlx::Error> for DatabaseError {
    fn from(err: sqlx::Error) -> Self {
        Self::new(err)
    }
}

/// An invalid state machine transition was attempted.
#[derive(Debug, Error, Serialize, Clone, PartialEq, Eq)]
#[error("invalid transition from '{from}' to '{to}'")]
pub struct InvalidTransitionError {
    pub from: String,
    pub to: String,
}

impl InvalidTransitionError {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

// ── Top-level error enum ──────────────────────────────────────────────────────

/// The root error type for KOVA services.
///
/// Each variant maps to an HTTP status code via [`KovaError::http_status`].
#[derive(Debug, Error, Serialize)]
pub enum KovaError {
    #[error(transparent)]
    Database(#[from] DatabaseError),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("{entity} not found: {id}")]
    NotFound {
        entity: &'static str,
        id: String,
    },

    #[error("insufficient funds")]
    InsufficientFunds,

    #[error(transparent)]
    InvalidTransition(#[from] InvalidTransitionError),

    #[error("idempotency conflict for key: {key}")]
    IdempotencyConflict { key: String },

    #[error("authentication error")]
    Auth,

    #[error("payment blocked by fraud system")]
    FraudBlock,
}

impl KovaError {
    /// Map this error to an HTTP status code for REST response serialisation.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::Database(_) => 500,
            Self::Validation(_) => 400,
            Self::NotFound { .. } => 404,
            Self::InsufficientFunds => 422,
            Self::InvalidTransition(_) => 422,
            Self::IdempotencyConflict { .. } => 409,
            Self::Auth => 401,
            Self::FraudBlock => 403,
        }
    }

    /// Convenience constructors.

    pub fn not_found(entity: &'static str, id: impl std::fmt::Display) -> Self {
        Self::NotFound {
            entity,
            id: id.to_string(),
        }
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    pub fn idempotency_conflict(key: impl Into<String>) -> Self {
        Self::IdempotencyConflict { key: key.into() }
    }
}

// ── From impls for convenience ────────────────────────────────────────────────

impl From<sqlx::Error> for KovaError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(DatabaseError::new(err))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_codes() {
        assert_eq!(KovaError::Auth.http_status(), 401);
        assert_eq!(KovaError::FraudBlock.http_status(), 403);
        assert_eq!(KovaError::not_found("user", "123").http_status(), 404);
        assert_eq!(KovaError::validation("bad input").http_status(), 400);
        assert_eq!(KovaError::InsufficientFunds.http_status(), 422);
        assert_eq!(KovaError::idempotency_conflict("key-1").http_status(), 409);
        assert_eq!(
            KovaError::Database(DatabaseError::with_message("oops")).http_status(),
            500
        );
    }

    #[test]
    fn invalid_transition_error_message() {
        let e = InvalidTransitionError::new("Settled", "Initiated");
        assert_eq!(e.to_string(), "invalid transition from 'Settled' to 'Initiated'");
    }

    #[test]
    fn database_error_hides_source() {
        let e = DatabaseError::new(sqlx::Error::RowNotFound);
        let json = serde_json::to_string(&e).unwrap();
        // The generic safe message is serialised, not the SQL error details
        assert!(json.contains("database error occurred"), "json: {json}");
        // The inner sqlx::Error is skipped — no SQL details in output
        assert!(!json.contains("RowNotFound"), "sql detail leaked: {json}");
        assert!(!json.contains("source"), "source field leaked: {json}");
    }

    #[test]
    fn kova_error_from_sqlx_error() {
        // Verify the From impl compiles and produces a Database variant.
        let sqlx_err = sqlx::Error::RowNotFound;
        let kova_err = KovaError::from(sqlx_err);
        assert!(matches!(kova_err, KovaError::Database(_)));
        assert_eq!(kova_err.http_status(), 500);
    }

    #[test]
    fn not_found_formats_entity_and_id() {
        let e = KovaError::not_found("account", "abc-123");
        assert_eq!(e.to_string(), "account not found: abc-123");
    }
}
