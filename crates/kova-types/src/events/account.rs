//! Kafka event structs for `kova.account.*` topics (TASK-022).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    entities::CurrencyCode,
    ids::{KovaAccountId, KovaUserId},
};

// ── Topic constants ───────────────────────────────────────────────────────────

pub const TOPIC_ACCOUNT_CREATED: &str = "kova.account.created";
pub const TOPIC_ACCOUNT_ACTIVATED: &str = "kova.account.activated";
pub const TOPIC_ACCOUNT_SUSPENDED: &str = "kova.account.suspended";
pub const TOPIC_ACCOUNT_CLOSED: &str = "kova.account.closed";
pub const TOPIC_ACCOUNT_LIMIT_CHANGED: &str = "kova.account.limit_changed";

pub const SCHEMA_VERSION: u8 = 1;

// ── Event structs ─────────────────────────────────────────────────────────────

/// A new account was created (status = `PendingActivation`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountCreatedEvent {
    pub schema_version: u8,
    pub event_id: KovaAccountId,
    pub account_id: KovaAccountId,
    pub user_id: KovaUserId,
    pub currency: CurrencyCode,
    pub account_number: String,
    pub occurred_at: DateTime<Utc>,
}

/// Account moved from `PendingActivation` → `Active`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountActivatedEvent {
    pub schema_version: u8,
    pub event_id: KovaAccountId,
    pub account_id: KovaAccountId,
    pub user_id: KovaUserId,
    pub activated_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Account suspended due to fraud review, regulatory hold, or user request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountSuspendedEvent {
    pub schema_version: u8,
    pub event_id: KovaAccountId,
    pub account_id: KovaAccountId,
    pub user_id: KovaUserId,
    pub reason: String,
    pub suspended_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Account permanently closed — terminal state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountClosedEvent {
    pub schema_version: u8,
    pub event_id: KovaAccountId,
    pub account_id: KovaAccountId,
    pub user_id: KovaUserId,
    pub reason: String,
    pub closed_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Daily or single-transaction spending limit was updated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountLimitChangedEvent {
    pub schema_version: u8,
    pub event_id: KovaAccountId,
    pub account_id: KovaAccountId,
    pub user_id: KovaUserId,
    /// `"daily_spend"` or `"single_transaction"`.
    pub limit_type: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub old_limit: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub new_limit: Decimal,
    pub currency: CurrencyCode,
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

    fn aid() -> KovaAccountId {
        KovaAccountId::from(Uuid::now_v7())
    }

    fn uid() -> KovaUserId {
        KovaUserId::from(Uuid::now_v7())
    }

    #[test]
    fn account_created_roundtrip() {
        let ev = AccountCreatedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: aid(),
            account_id: aid(),
            user_id: uid(),
            currency: CurrencyCode::NGN,
            account_number: "9012345678".to_string(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""schema_version":1"#));
        let back: AccountCreatedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn account_limit_changed_roundtrip() {
        let ev = AccountLimitChangedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: aid(),
            account_id: aid(),
            user_id: uid(),
            limit_type: "daily_spend".to_string(),
            old_limit: dec!(500000.0000),
            new_limit: dec!(1000000.0000),
            currency: CurrencyCode::NGN,
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""old_limit":"500000.0000""#));
        assert!(json.contains(r#""new_limit":"1000000.0000""#));
        let back: AccountLimitChangedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn account_suspended_roundtrip() {
        let ev = AccountSuspendedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: aid(),
            account_id: aid(),
            user_id: uid(),
            reason: "Fraud hold".to_string(),
            suspended_at: now(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: AccountSuspendedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
