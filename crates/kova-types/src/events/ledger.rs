//! Kafka event structs for `kova.ledger.*` topics (TASK-026).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    entities::CurrencyCode,
    ids::{KovaAccountId, KovaLedgerEntryId, KovaPaymentId, KovaUserId},
};

// ── Topic constants ───────────────────────────────────────────────────────────

pub const TOPIC_LEDGER_ENTRY_CREATED: &str = "kova.ledger.entry_created";
pub const TOPIC_LEDGER_RECONCILIATION: &str = "kova.ledger.reconciliation";

pub const SCHEMA_VERSION: u8 = 1;

// ── Event structs ─────────────────────────────────────────────────────────────

/// A double-entry ledger pair was posted. Both debit and credit entries share
/// the same `payment_id` and `correlation_id`, allowing consumers to join the
/// two legs without querying the ledger service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntryCreatedEvent {
    pub schema_version: u8,
    pub event_id: KovaLedgerEntryId,
    pub entry_id: KovaLedgerEntryId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    pub debit_account_id: KovaAccountId,
    pub credit_account_id: KovaAccountId,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub currency: CurrencyCode,
    /// `"payment"`, `"refund"`, `"fx_conversion"`, `"fee"`, `"reversal"`.
    pub entry_type: String,
    pub occurred_at: DateTime<Utc>,
}

/// Periodic reconciliation completed — summary of the reconciliation window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerReconciliationEvent {
    pub schema_version: u8,
    pub event_id: KovaLedgerEntryId,
    pub currency: CurrencyCode,
    /// Number of double-entry pairs verified in this reconciliation run.
    pub entries_checked: u64,
    /// Number of entries where debit != credit (should be zero in steady state).
    pub discrepancies_found: u32,
    #[serde(with = "rust_decimal::serde::str")]
    pub total_debits: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub total_credits: Decimal,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn lid() -> KovaLedgerEntryId {
        KovaLedgerEntryId::from(Uuid::now_v7())
    }

    #[test]
    fn ledger_entry_created_roundtrip() {
        let ev = LedgerEntryCreatedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: lid(),
            entry_id: lid(),
            payment_id: KovaPaymentId::from(Uuid::now_v7()),
            user_id: KovaUserId::from(Uuid::now_v7()),
            debit_account_id: KovaAccountId::from(Uuid::now_v7()),
            credit_account_id: KovaAccountId::from(Uuid::now_v7()),
            amount: dec!(7500.0000),
            currency: CurrencyCode::NGN,
            entry_type: "payment".to_string(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""amount":"7500.0000""#));
        assert!(json.contains(r#""schema_version":1"#));
        let back: LedgerEntryCreatedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn ledger_reconciliation_roundtrip() {
        let ev = LedgerReconciliationEvent {
            schema_version: SCHEMA_VERSION,
            event_id: lid(),
            currency: CurrencyCode::NGN,
            entries_checked: 50_000,
            discrepancies_found: 0,
            total_debits: dec!(12_345_678.0000),
            total_credits: dec!(12_345_678.0000),
            window_start: DateTime::from_timestamp(1_699_990_000, 0).unwrap(),
            window_end: now(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""discrepancies_found":0"#));
        let back: LedgerReconciliationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn ledger_entry_amount_is_string_not_number() {
        let ev = LedgerEntryCreatedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: lid(),
            entry_id: lid(),
            payment_id: KovaPaymentId::from(Uuid::now_v7()),
            user_id: KovaUserId::from(Uuid::now_v7()),
            debit_account_id: KovaAccountId::from(Uuid::now_v7()),
            credit_account_id: KovaAccountId::from(Uuid::now_v7()),
            amount: dec!(0.0001),
            currency: CurrencyCode::USD,
            entry_type: "fee".to_string(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        // Must NOT appear as JSON number
        assert!(!json.contains(r#""amount":0.0001"#), "amount serialized as number");
        assert!(json.contains(r#""amount":"0.0001""#));
    }
}
