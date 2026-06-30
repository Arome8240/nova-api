//! Kafka event structs for `kova.card.*` topics (TASK-023).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    entities::CurrencyCode,
    ids::{KovaAccountId, KovaCardId, KovaUserId},
};

// ── Topic constants ───────────────────────────────────────────────────────────

pub const TOPIC_CARD_ISSUED: &str = "kova.card.issued";
pub const TOPIC_CARD_ACTIVATED: &str = "kova.card.activated";
pub const TOPIC_CARD_BLOCKED: &str = "kova.card.blocked";
pub const TOPIC_CARD_UNBLOCKED: &str = "kova.card.unblocked";
pub const TOPIC_CARD_EXPIRED: &str = "kova.card.expired";
pub const TOPIC_CARD_TRANSACTION_APPROVED: &str = "kova.card.transaction.approved";
pub const TOPIC_CARD_TRANSACTION_DECLINED: &str = "kova.card.transaction.declined";

pub const SCHEMA_VERSION: u8 = 1;

// ── Enums ─────────────────────────────────────────────────────────────────────

/// Reasons a card transaction was declined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclineReason {
    InsufficientFunds,
    CardBlocked,
    CardExpired,
    DailyLimitExceeded,
    FraudBlock,
    InvalidPin,
    MerchantBlocked,
    Other,
}

// ── Event structs ─────────────────────────────────────────────────────────────

/// A new virtual or physical card was issued.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardIssuedEvent {
    pub schema_version: u8,
    pub event_id: KovaCardId,
    pub card_id: KovaCardId,
    pub user_id: KovaUserId,
    pub account_id: KovaAccountId,
    /// `"virtual"` or `"physical"`.
    pub card_type: String,
    pub last_four: String,
    pub issued_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Card PIN set and card activated for use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardActivatedEvent {
    pub schema_version: u8,
    pub event_id: KovaCardId,
    pub card_id: KovaCardId,
    pub user_id: KovaUserId,
    pub activated_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Card blocked by user request or fraud system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardBlockedEvent {
    pub schema_version: u8,
    pub event_id: KovaCardId,
    pub card_id: KovaCardId,
    pub user_id: KovaUserId,
    pub reason: String,
    pub blocked_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Blocked card unblocked by user request (not applicable to expired/cancelled cards).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardUnblockedEvent {
    pub schema_version: u8,
    pub event_id: KovaCardId,
    pub card_id: KovaCardId,
    pub user_id: KovaUserId,
    pub occurred_at: DateTime<Utc>,
}

/// Card passed its expiry date — terminal state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardExpiredEvent {
    pub schema_version: u8,
    pub event_id: KovaCardId,
    pub card_id: KovaCardId,
    pub user_id: KovaUserId,
    pub expired_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// Card transaction authorized and debit reserved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardTransactionApprovedEvent {
    pub schema_version: u8,
    pub event_id: KovaCardId,
    pub card_id: KovaCardId,
    pub user_id: KovaUserId,
    pub merchant_name: String,
    pub merchant_category_code: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub currency: CurrencyCode,
    pub occurred_at: DateTime<Utc>,
}

/// Card transaction declined at point of sale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardTransactionDeclinedEvent {
    pub schema_version: u8,
    pub event_id: KovaCardId,
    pub card_id: KovaCardId,
    pub user_id: KovaUserId,
    pub merchant_name: String,
    pub merchant_category_code: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub currency: CurrencyCode,
    pub decline_reason: DeclineReason,
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

    fn cid() -> KovaCardId {
        KovaCardId::from(Uuid::now_v7())
    }

    fn uid() -> KovaUserId {
        KovaUserId::from(Uuid::now_v7())
    }

    #[test]
    fn card_issued_roundtrip() {
        let ev = CardIssuedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: cid(),
            card_id: cid(),
            user_id: uid(),
            account_id: KovaAccountId::from(Uuid::now_v7()),
            card_type: "virtual".to_string(),
            last_four: "4242".to_string(),
            issued_at: now(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""schema_version":1"#));
        let back: CardIssuedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn decline_reason_serializes_snake_case() {
        let ev = CardTransactionDeclinedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: cid(),
            card_id: cid(),
            user_id: uid(),
            merchant_name: "Test Shop".to_string(),
            merchant_category_code: "5411".to_string(),
            amount: dec!(20.0000),
            currency: CurrencyCode::GBP,
            decline_reason: DeclineReason::InsufficientFunds,
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(
            json.contains(r#""decline_reason":"insufficient_funds""#),
            "DeclineReason not snake_case: {json}"
        );
        let back: CardTransactionDeclinedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn card_transaction_approved_amount_is_string() {
        let ev = CardTransactionApprovedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: cid(),
            card_id: cid(),
            user_id: uid(),
            merchant_name: "Shoprite".to_string(),
            merchant_category_code: "5411".to_string(),
            amount: dec!(3500.0000),
            currency: CurrencyCode::NGN,
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""amount":"3500.0000""#));
        let back: CardTransactionApprovedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
