//! Kafka event structs for `kova.fraud.*` topics (TASK-025).
//!
//! `risk_score` is a 0-100 integer stored as `u8` with a custom deserializer
//! that rejects values outside [0, 100].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

use crate::ids::{KovaPaymentId, KovaUserId};

// ── Topic constants ───────────────────────────────────────────────────────────

pub const TOPIC_FRAUD_DECISION: &str = "kova.fraud.decision";
pub const TOPIC_FRAUD_CASE_CREATED: &str = "kova.fraud.case_created";

pub const SCHEMA_VERSION: u8 = 1;

// ── Enums ─────────────────────────────────────────────────────────────────────

/// Outcome of a fraud evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FraudDecision {
    Allow,
    Review,
    Block,
}

// ── Custom deserializer for risk_score ───────────────────────────────────────

fn deserialize_risk_score<'de, D>(de: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let v = u8::deserialize(de)?;
    if v > 100 {
        return Err(serde::de::Error::custom(format!(
            "risk_score must be 0-100, got {v}"
        )));
    }
    Ok(v)
}

// ── Event structs ─────────────────────────────────────────────────────────────

/// Fraud decision returned for a payment or card transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FraudDecisionEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    pub decision: FraudDecision,
    /// 0 (clean) – 100 (definite fraud). Validated on deserialization.
    #[serde(deserialize_with = "deserialize_risk_score")]
    pub risk_score: u8,
    /// Human-readable summary of the model's rationale.
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

/// A fraud case was opened for manual review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FraudCaseCreatedEvent {
    pub schema_version: u8,
    pub event_id: KovaPaymentId,
    pub case_id: String,
    pub payment_id: KovaPaymentId,
    pub user_id: KovaUserId,
    #[serde(deserialize_with = "deserialize_risk_score")]
    pub risk_score: u8,
    pub assigned_to: Option<String>,
    pub created_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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
    fn fraud_decision_allow_roundtrip() {
        let ev = FraudDecisionEvent {
            schema_version: SCHEMA_VERSION,
            event_id: pid(),
            payment_id: pid(),
            user_id: uid(),
            decision: FraudDecision::Allow,
            risk_score: 5,
            reason: "Low velocity, known device".to_string(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        // FraudDecision must serialize lowercase
        assert!(json.contains(r#""decision":"allow""#), "{json}");
        let back: FraudDecisionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn fraud_decision_block_roundtrip() {
        let ev = FraudDecisionEvent {
            schema_version: SCHEMA_VERSION,
            event_id: pid(),
            payment_id: pid(),
            user_id: uid(),
            decision: FraudDecision::Block,
            risk_score: 95,
            reason: "Velocity pattern matches known fraud ring".to_string(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""decision":"block""#));
        let back: FraudDecisionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn risk_score_101_rejected() {
        let json = r#"{
            "schema_version":1,
            "event_id":"01900000-0000-7000-8000-000000000001",
            "payment_id":"01900000-0000-7000-8000-000000000002",
            "user_id":"01900000-0000-7000-8000-000000000003",
            "decision":"allow",
            "risk_score":101,
            "reason":"test",
            "occurred_at":"2023-11-14T22:13:20Z"
        }"#;
        let result: Result<FraudDecisionEvent, _> = serde_json::from_str(json);
        assert!(result.is_err(), "risk_score=101 should fail deserialization");
    }

    #[test]
    fn risk_score_100_accepted() {
        let json = r#"{
            "schema_version":1,
            "event_id":"01900000-0000-7000-8000-000000000001",
            "payment_id":"01900000-0000-7000-8000-000000000002",
            "user_id":"01900000-0000-7000-8000-000000000003",
            "decision":"block",
            "risk_score":100,
            "reason":"confirmed fraud",
            "occurred_at":"2023-11-14T22:13:20Z"
        }"#;
        let ev: FraudDecisionEvent = serde_json::from_str(json).unwrap();
        assert_eq!(ev.risk_score, 100);
    }

    #[test]
    fn fraud_case_created_roundtrip() {
        let ev = FraudCaseCreatedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: pid(),
            case_id: "CASE-001".to_string(),
            payment_id: pid(),
            user_id: uid(),
            risk_score: 72,
            assigned_to: Some("compliance@kova.com".to_string()),
            created_at: now(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: FraudCaseCreatedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
