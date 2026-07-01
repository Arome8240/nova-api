//! External service client traits and stub implementations.
//!
//! Real implementations will use tonic gRPC when kova-ledger and kova-payments
//! are built. The stubs allow the service to compile and unit-test without
//! those services existing.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde_json::Value;

use kova_types::{entities::CurrencyCode, ids::KovaAccountId};

use crate::error::AccountError;

// ── Balance / ledger types ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BalanceResult {
    pub available_balance: Decimal,
    pub pending_amount: Decimal,
    pub currency: CurrencyCode,
}

#[derive(Debug, Clone)]
pub struct LedgerEntry {
    pub entry_id: String,
    pub amount: Decimal,
    pub direction: String, // "credit" | "debit"
    pub description: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct StatementPage {
    pub entries: Vec<LedgerEntry>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

// ── LedgerClient ─────────────────────────────────────────────────────────────

#[async_trait]
pub trait LedgerClient: Send + Sync {
    /// Create the opening zero-balance ledger entry for a new account.
    async fn create_opening_entry(
        &self,
        account_id: KovaAccountId,
        currency: CurrencyCode,
    ) -> Result<(), AccountError>;

    /// Fetch the current balance. Never reads from kova_account_db.
    async fn get_balance(&self, account_id: KovaAccountId) -> Result<BalanceResult, AccountError>;

    /// Fetch a page of ledger entries with keyset cursor pagination.
    async fn get_statement(
        &self,
        account_id: KovaAccountId,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<StatementPage, AccountError>;
}

/// Stub used until kova-ledger gRPC is wired in.
pub struct LedgerStub;

#[async_trait]
impl LedgerClient for LedgerStub {
    async fn create_opening_entry(
        &self,
        account_id: KovaAccountId,
        _currency: CurrencyCode,
    ) -> Result<(), AccountError> {
        tracing::debug!(%account_id, "LedgerStub: create_opening_entry (no-op)");
        Ok(())
    }

    async fn get_balance(&self, account_id: KovaAccountId) -> Result<BalanceResult, AccountError> {
        tracing::debug!(%account_id, "LedgerStub: get_balance → returning zeros");
        Ok(BalanceResult {
            available_balance: Decimal::ZERO,
            pending_amount: Decimal::ZERO,
            currency: CurrencyCode::NGN,
        })
    }

    async fn get_statement(
        &self,
        account_id: KovaAccountId,
        _cursor: Option<String>,
        _limit: u32,
    ) -> Result<StatementPage, AccountError> {
        tracing::debug!(%account_id, "LedgerStub: get_statement → returning empty page");
        Ok(StatementPage {
            entries: vec![],
            next_cursor: None,
            has_more: false,
        })
    }
}

// ── PaymentsClient ────────────────────────────────────────────────────────────

#[async_trait]
pub trait PaymentsClient: Send + Sync {
    /// Returns true if the account has any pending (unsettled) payments.
    async fn has_pending_payments(
        &self,
        account_id: KovaAccountId,
    ) -> Result<bool, AccountError>;
}

/// Stub used until kova-payments gRPC is wired in.
pub struct PaymentsStub;

#[async_trait]
impl PaymentsClient for PaymentsStub {
    async fn has_pending_payments(
        &self,
        account_id: KovaAccountId,
    ) -> Result<bool, AccountError> {
        tracing::debug!(%account_id, "PaymentsStub: has_pending_payments → false");
        Ok(false)
    }
}

// ── EventPublisher ────────────────────────────────────────────────────────────

#[async_trait]
pub trait EventPublisher: Send + Sync {
    /// Publish a pre-serialised JSON event to a Kafka topic.
    async fn publish_json(
        &self,
        topic: &str,
        key: &str,
        payload: Value,
    ) -> Result<(), AccountError>;
}

/// No-op publisher used until TASK-028 (outbox pattern) wires in Kafka.
pub struct NoOpPublisher;

#[async_trait]
impl EventPublisher for NoOpPublisher {
    async fn publish_json(&self, topic: &str, key: &str, _payload: Value) -> Result<(), AccountError> {
        tracing::debug!(topic, key, "NoOpPublisher: event dropped (no Kafka configured)");
        Ok(())
    }
}
