//! Kafka event structs for `kova.payment.*` topics (TASK-021).
//!
//! Consumers must NOT use `#[serde(deny_unknown_fields)]` — fields may be
//! added in future schema versions. Check `schema_version` to handle evolution.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    entities::CurrencyCode,
    ids::{KovaAccountId, KovaPaymentId, KovaUserId},
};

// ── Topic constants ───────────────────────────────────────────────────────────

pub const TOPIC_PAYMENT_INITIATED: &str = "kova.payment.initiated";
pub const TOPIC_PAYMENT_VALIDATED: &str = "kova.payment.validated";
pub const TOPIC_PAYMENT_FRAUD_CHECKED: &str = "kova.payment.fraud_checked";
pub const TOPIC_PAYMENT_FX_CONVERTED: &str = "kova.payment.fx_converted";
pub const TOPIC_PAYMENT_SETTLED: &str = "kova.payment.settled";
pub const TOPIC_PAYMENT_FAILED: &str = "kova.payment.failed";
pub const TOPIC_PAYMENT_REFUNDED: &str = "kova.payment.refunded";
pub const TOPIC_PAYMENT_EXPIRED: &str = "kova.payment.expired";

pub const SCHEMA_VERSION: u8 = 1;

// ── Event structs ─────────────────────────────────────────────────────────────

/// A payment was created and entered the `Initiated` state.
/// Kafka key: `payment_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentInitiatedEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    pub source_account_id: KovaAccountId,
    pub destination_account_id: KovaAccountId,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub currency: CurrencyCode,
    /// Payment rail identifier, e.g. `"NIBSS"`, `"FasterPayments"`.
    pub rail: String,
    pub idempotency_key: String,
    pub occurred_at: DateTime<Utc>,
}

/// Input validation completed (passed or failed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentValidatedEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    pub validation_passed: bool,
    /// Present only when `validation_passed` is false.
    pub failure_reason: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

/// Fraud evaluation completed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentFraudCheckedEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    /// `"Allow"`, `"Review"`, or `"Block"`.
    pub decision: String,
    /// 0 (clean) – 100 (definite fraud).
    pub risk_score: u8,
    pub occurred_at: DateTime<Utc>,
}

/// FX conversion completed; cross-currency amount locked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentFxConvertedEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    pub from_currency: CurrencyCode,
    pub to_currency: CurrencyCode,
    #[serde(with = "rust_decimal::serde::str")]
    pub rate: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub source_amount: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub converted_amount: Decimal,
    pub occurred_at: DateTime<Utc>,
}

/// Payment settled successfully — terminal success state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentSettledEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub currency: CurrencyCode,
    /// External reference returned by the payment rail.
    pub rail_reference: String,
    pub settled_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Payment failed at any stage — terminal failure state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentFailedEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    pub reason: String,
    pub failed_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// A settled payment was refunded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentRefundedEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub currency: CurrencyCode,
    pub refunded_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Payment timed out — terminal expiry state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentExpiredEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    pub expired_at: DateTime<Utc>,
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

    fn pid() -> KovaPaymentId {
        KovaPaymentId::from(Uuid::now_v7())
    }

    fn uid() -> KovaUserId {
        KovaUserId::from(Uuid::now_v7())
    }

    #[test]
    fn payment_initiated_roundtrip() {
        let ev = PaymentInitiatedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: pid(),
            payment_id: pid(),
            user_id: uid(),
            source_account_id: KovaAccountId::from(Uuid::now_v7()),
            destination_account_id: KovaAccountId::from(Uuid::now_v7()),
            amount: dec!(1250.0000),
            currency: CurrencyCode::NGN,
            rail: "NIBSS".to_string(),
            idempotency_key: "key-001".to_string(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        // Amount must be a string, not a number
        assert!(json.contains(r#""amount":"1250.0000""#), "amount not a string: {json}");
        // ISO 8601 timestamp, not an integer
        assert!(json.contains("2023-11"), "occurred_at not ISO 8601: {json}");
        assert!(json.contains(r#""schema_version":1"#));
        let back: PaymentInitiatedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn payment_settled_roundtrip() {
        let ev = PaymentSettledEvent {
            schema_version: SCHEMA_VERSION,
            event_id: pid(),
            payment_id: pid(),
            user_id: uid(),
            amount: dec!(500.5000),
            currency: CurrencyCode::GBP,
            rail_reference: "FP-123".to_string(),
            settled_at: now(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""amount":"500.5000""#));
        let back: PaymentSettledEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn payment_fx_converted_roundtrip() {
        let ev = PaymentFxConvertedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: pid(),
            payment_id: pid(),
            user_id: uid(),
            from_currency: CurrencyCode::NGN,
            to_currency: CurrencyCode::GBP,
            rate: dec!(0.0005),
            source_amount: dec!(100000.0000),
            converted_amount: dec!(50.0000),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""rate":"0.0005""#));
        let back: PaymentFxConvertedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn payment_failed_roundtrip() {
        let ev = PaymentFailedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: pid(),
            payment_id: pid(),
            user_id: uid(),
            reason: "Rail timeout".to_string(),
            failed_at: now(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: PaymentFailedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
