//! Kafka event structs for `kova.notification.*` topics (TASK-027).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{KovaNotificationId, KovaUserId};

// ── Topic constants ───────────────────────────────────────────────────────────

pub const TOPIC_PUSH_NOTIFICATION_REQUESTED: &str = "kova.notification.push_requested";
pub const TOPIC_IN_APP_NOTIFICATION: &str = "kova.notification.in_app";

pub const SCHEMA_VERSION: u8 = 1;

// ── Enums ─────────────────────────────────────────────────────────────────────

/// Semantic category of a notification. Consumers use this to route to the
/// correct template and sound profile on the mobile client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    PaymentReceived,
    PaymentSent,
    PaymentFailed,
    CardTransactionApproved,
    CardTransactionDeclined,
    KycApproved,
    KycRejected,
    FraudAlert,
    LowBalance,
    SystemAnnouncement,
}

// ── Event structs ─────────────────────────────────────────────────────────────

/// Request to push a notification to a user's registered devices.
/// Published by any service; consumed by kova-notifications.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushNotificationRequestedEvent {
    pub schema_version: u8,
    pub event_id: KovaNotificationId,
    pub user_id: KovaUserId,
    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,
    /// Optional deep-link path, e.g. `/payments/01ABCDEF...`.
    pub deep_link: Option<String>,
    /// Optional arbitrary payload to attach to the push (max 4 KB on APNs).
    pub extra: Option<serde_json::Value>,
    pub occurred_at: DateTime<Utc>,
}

/// Notification to persist in the in-app inbox (visible in the KOVA app).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InAppNotificationEvent {
    pub schema_version: u8,
    pub event_id: KovaNotificationId,
    pub user_id: KovaUserId,
    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,
    /// Optional deep-link path.
    pub deep_link: Option<String>,
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

    fn nid() -> KovaNotificationId {
        KovaNotificationId::from(Uuid::now_v7())
    }

    fn uid() -> KovaUserId {
        KovaUserId::from(Uuid::now_v7())
    }

    #[test]
    fn push_notification_roundtrip() {
        let ev = PushNotificationRequestedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: nid(),
            user_id: uid(),
            notification_type: NotificationType::PaymentReceived,
            title: "Money received!".to_string(),
            body: "₦5,000 received from John".to_string(),
            deep_link: Some("/payments/01ABCDEF".to_string()),
            extra: Some(serde_json::json!({"payment_id": "01ABCDEF"})),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""notification_type":"payment_received""#), "{json}");
        assert!(json.contains(r#""schema_version":1"#));
        let back: PushNotificationRequestedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn in_app_notification_roundtrip() {
        let ev = InAppNotificationEvent {
            schema_version: SCHEMA_VERSION,
            event_id: nid(),
            user_id: uid(),
            notification_type: NotificationType::KycApproved,
            title: "KYC Approved".to_string(),
            body: "Your identity has been verified.".to_string(),
            deep_link: None,
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""notification_type":"kyc_approved""#));
        let back: InAppNotificationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn notification_type_fraud_alert_snake_case() {
        let t = NotificationType::FraudAlert;
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#""fraud_alert""#);
    }

    #[test]
    fn push_notification_no_deep_link_roundtrip() {
        let ev = PushNotificationRequestedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: nid(),
            user_id: uid(),
            notification_type: NotificationType::SystemAnnouncement,
            title: "Scheduled maintenance".to_string(),
            body: "KOVA will be down 02:00–03:00 WAT on 2026-07-01".to_string(),
            deep_link: None,
            extra: None,
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: PushNotificationRequestedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
