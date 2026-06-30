//! Core domain entity structs for KOVA.
//!
//! Each struct derives `sqlx::FromRow` for direct MySQL row mapping.
//! Column names in MySQL must match Rust field names (snake_case), or
//! `#[sqlx(rename = "column_name")]` overrides the mapping.
//!
//! # Monetary fields
//!
//! Amounts are stored as `rust_decimal::Decimal` (DECIMAL(19,4) in MySQL).
//! The currency is a separate `CurrencyCode` column. Never use `f64`/`f32`.
//!
//! # Timestamps
//!
//! All timestamps are `chrono::DateTime<chrono::Utc>` mapping to MySQL
//! `DATETIME` or `DATETIME(6)` columns.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::{
    ids::{KovaAccountId, KovaAuditEventId, KovaCardId, KovaFxQuoteId, KovaLedgerEntryId, KovaPaymentId, KovaUserId},
    kyc::KycStatus,
};

// ── CurrencyCode ──────────────────────────────────────────────────────────────

/// Runtime currency discriminant — used in entity structs where the currency
/// is stored as a MySQL ENUM column and known only at query time.
///
/// For compile-time phantom-typed money arithmetic, use [`crate::money::Money<C>`]
/// with the corresponding marker type (`NGN`, `GBP`, `USD`, `KES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CurrencyCode {
    NGN,
    GBP,
    USD,
    KES,
}

impl_sqlx_string_enum!(CurrencyCode,
    NGN => "NGN",
    GBP => "GBP",
    USD => "USD",
    KES => "KES",
);

// ── AccountStatus ─────────────────────────────────────────────────────────────

/// Lifecycle state of a KOVA account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccountStatus {
    /// Account is open and fully operational.
    Active,
    /// Account is temporarily suspended (no debits, no credits).
    Frozen,
    /// Account is permanently closed.
    Closed,
    /// Account is created but blocked pending KYC approval.
    PendingKYC,
}

impl_sqlx_string_enum!(AccountStatus,
    Active     => "Active",
    Frozen     => "Frozen",
    Closed     => "Closed",
    PendingKYC => "PendingKYC",
);

// ── CardStatus ────────────────────────────────────────────────────────────────

/// Lifecycle state of a KOVA virtual/physical card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CardStatus {
    /// Card is active and can authorise transactions.
    Active,
    /// Card is temporarily frozen — no new authorisations.
    Frozen,
    /// Card is permanently cancelled.
    Cancelled,
}

impl_sqlx_string_enum!(CardStatus,
    Active    => "Active",
    Frozen    => "Frozen",
    Cancelled => "Cancelled",
);

// ── User ──────────────────────────────────────────────────────────────────────

/// A KOVA user account.
///
/// Maps to `kova_auth_db.users`. The PIN hash is argon2id — never decoded
/// for non-auth purposes. Include it in queries only when the service needs
/// to verify credentials.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: KovaUserId,
    pub phone_number: String,
    /// argon2id hash — never log or serialize to external APIs.
    #[serde(skip_serializing)]
    pub pin_hash: String,
    pub kyc_status: KycStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── Account ───────────────────────────────────────────────────────────────────

/// A KOVA bank account.
///
/// Maps to `kova_account_db.accounts`. There is deliberately **no `balance`
/// field** — balance is always derived from `kova-ledger`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Account {
    pub id: KovaAccountId,
    pub user_id: KovaUserId,
    pub account_number: String,
    pub currency: CurrencyCode,
    pub status: AccountStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

// ── Card ──────────────────────────────────────────────────────────────────────

/// A KOVA virtual or physical payment card.
///
/// Maps to `kova_cards_db.cards`. The raw PAN is never stored — only the
/// `vault_token` (an HMAC-derived reference to the external card vault).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Card {
    pub id: KovaCardId,
    pub account_id: KovaAccountId,
    pub user_id: KovaUserId,
    /// External vault reference — never a raw PAN.
    pub vault_token: String,
    /// Last four digits of the card number, for display only.
    pub last_four: String,
    pub expiry_month: u8,
    pub expiry_year: u16,
    pub status: CardStatus,
    /// Per-card spending controls (daily limit, MCC restrictions, etc.).
    pub spend_controls: Option<JsonValue>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── LedgerEntry ───────────────────────────────────────────────────────────────

/// A double-entry ledger posting.
///
/// Maps to `kova_ledger_db.ledger_entries`. Every posting is immutable
/// (enforced by a MySQL BEFORE UPDATE trigger). `amount` is always positive;
/// direction is determined by which account is debit vs. credit.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LedgerEntry {
    pub id: KovaLedgerEntryId,
    /// The account being debited.
    pub debit_account_id: KovaAccountId,
    /// The account being credited.
    pub credit_account_id: KovaAccountId,
    /// Always positive (DECIMAL(19,4)). Direction is encoded by debit/credit sides.
    pub amount: Decimal,
    pub currency: CurrencyCode,
    /// Prevents duplicate postings — must be unique in the ledger.
    pub idempotency_key: String,
    /// The payment that triggered this posting, if applicable.
    pub payment_id: Option<KovaPaymentId>,
    /// Microsecond-precision timestamp (DATETIME(6)) for correct ordering.
    pub created_at: DateTime<Utc>,
    pub metadata: Option<JsonValue>,
}

// ── FxQuote ───────────────────────────────────────────────────────────────────

/// A locked FX rate quote.
///
/// Stored in Redis for 30 seconds and consumed atomically by `kova-fx`.
/// Once consumed, a row is inserted into `kova_fx_db` for audit.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FxQuote {
    pub id: KovaFxQuoteId,
    /// Opaque token used to consume the quote via `kova-fx.ConsumeQuote` RPC.
    pub quote_token: String,
    /// When the locked rate expires (30 s after creation).
    pub expires_at: DateTime<Utc>,
    pub from_currency: CurrencyCode,
    pub to_currency: CurrencyCode,
    /// Units of `to_currency` per one unit of `from_currency`.
    pub rate: Decimal,
    /// Amount of `from_currency` locked at this rate.
    pub locked_amount: Decimal,
    pub created_at: DateTime<Utc>,
}

// ── AuditEvent ────────────────────────────────────────────────────────────────

/// An immutable audit trail entry.
///
/// Maps to `kova_audit_db.audit_events`. Populated from Kafka events by
/// `kova-audit`. UPDATE and DELETE are blocked by MySQL triggers.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuditEvent {
    pub id: KovaAuditEventId,
    /// Kafka topic name or internal event classifier, e.g. `"kova.payment.completed"`.
    pub event_type: String,
    /// The kind of entity this event concerns, e.g. `"payment"`, `"account"`.
    pub entity_type: String,
    /// The UUID of the affected entity (any entity type).
    pub entity_id: Uuid,
    /// The user or service that caused the event. `None` for system-initiated events.
    pub actor_id: Option<Uuid>,
    /// Full event payload — never redact at the audit layer.
    pub payload: JsonValue,
    /// SHA-256 hash of the previous event (for chain integrity). `None` for the first event.
    pub previous_hash: Option<Vec<u8>>,
    /// SHA-256 hash of this event's canonical representation.
    pub event_hash: Option<Vec<u8>>,
    /// Microsecond-precision timestamp.
    pub created_at: DateTime<Utc>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn currency_code_display() {
        assert_eq!(CurrencyCode::NGN.to_string(), "NGN");
        assert_eq!(CurrencyCode::GBP.to_string(), "GBP");
    }

    #[test]
    fn currency_code_from_str() {
        assert_eq!(CurrencyCode::from_str("NGN").unwrap(), CurrencyCode::NGN);
        assert!(CurrencyCode::from_str("ngn").is_err()); // case-sensitive
    }

    #[test]
    fn account_status_display() {
        assert_eq!(AccountStatus::PendingKYC.to_string(), "PendingKYC");
        assert_eq!(AccountStatus::Active.to_string(), "Active");
    }

    #[test]
    fn card_status_display() {
        assert_eq!(CardStatus::Cancelled.to_string(), "Cancelled");
    }

    #[test]
    fn currency_code_serde_roundtrip() {
        let c = CurrencyCode::KES;
        let json = serde_json::to_string(&c).unwrap();
        let back: CurrencyCode = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn account_status_all_variants_round_trip() {
        for s in [
            AccountStatus::Active,
            AccountStatus::Frozen,
            AccountStatus::Closed,
            AccountStatus::PendingKYC,
        ] {
            let displayed = s.to_string();
            let parsed = AccountStatus::from_str(&displayed).unwrap();
            assert_eq!(s, parsed);
        }
    }
}
