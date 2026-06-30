//! Kafka event structs for `kova.kyc.*` topics (TASK-024).
//!
//! Note: this file is `events/kyc.rs`, distinct from the top-level `kyc.rs`
//! which defines status/risk enums. No naming conflict exists.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    ids::{KovaKycDocumentId, KovaUserId},
    kyc::KycRiskLevel,
};

// ── Topic constants ───────────────────────────────────────────────────────────

pub const TOPIC_KYC_SUBMITTED: &str = "kova.kyc.submitted";
pub const TOPIC_KYC_DOCUMENT_UPLOADED: &str = "kova.kyc.document_uploaded";
pub const TOPIC_KYC_REVIEW_STARTED: &str = "kova.kyc.review_started";
pub const TOPIC_KYC_APPROVED: &str = "kova.kyc.approved";
pub const TOPIC_KYC_REJECTED: &str = "kova.kyc.rejected";

pub const SCHEMA_VERSION: u8 = 1;

// ── Event structs ─────────────────────────────────────────────────────────────

/// User submitted a KYC application — status becomes `Submitted`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KycSubmittedEvent {
    pub schema_version: u8,
    pub event_id: KovaUserId,
    pub user_id: KovaUserId,
    pub application_id: KovaKycDocumentId,
    pub submitted_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// A KYC document (ID card, selfie, proof of address) was uploaded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KycDocumentUploadedEvent {
    pub schema_version: u8,
    pub event_id: KovaKycDocumentId,
    pub document_id: KovaKycDocumentId,
    pub user_id: KovaUserId,
    /// `"national_id"`, `"passport"`, `"selfie"`, `"proof_of_address"`, etc.
    pub document_type: String,
    pub uploaded_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// A compliance officer or automated workflow started reviewing the application.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KycReviewStartedEvent {
    pub schema_version: u8,
    pub event_id: KovaUserId,
    pub user_id: KovaUserId,
    pub application_id: KovaKycDocumentId,
    /// `None` for automated review; `Some` for human reviewer user ID.
    pub reviewer_id: Option<KovaUserId>,
    pub started_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// KYC approved — user can now transact. Carries final risk assessment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KycApprovedEvent {
    pub schema_version: u8,
    pub event_id: KovaUserId,
    pub user_id: KovaUserId,
    pub application_id: KovaKycDocumentId,
    pub risk_level: KycRiskLevel,
    pub approved_at: DateTime<Utc>,
    pub occurred_at: DateTime<Utc>,
}

/// KYC rejected — user cannot proceed until they resubmit (if permitted).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KycRejectedEvent {
    pub schema_version: u8,
    pub event_id: KovaUserId,
    pub user_id: KovaUserId,
    pub application_id: KovaKycDocumentId,
    pub rejection_reason: String,
    pub can_resubmit: bool,
    pub rejected_at: DateTime<Utc>,
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

    fn uid() -> KovaUserId {
        KovaUserId::from(Uuid::now_v7())
    }

    fn did() -> KovaKycDocumentId {
        KovaKycDocumentId::from(Uuid::now_v7())
    }

    #[test]
    fn kyc_submitted_roundtrip() {
        let ev = KycSubmittedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: uid(),
            user_id: uid(),
            application_id: did(),
            submitted_at: now(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""schema_version":1"#));
        let back: KycSubmittedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn kyc_approved_roundtrip() {
        let ev = KycApprovedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: uid(),
            user_id: uid(),
            application_id: did(),
            risk_level: KycRiskLevel::Low,
            approved_at: now(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: KycApprovedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn kyc_rejected_roundtrip() {
        let ev = KycRejectedEvent {
            schema_version: SCHEMA_VERSION,
            event_id: uid(),
            user_id: uid(),
            application_id: did(),
            rejection_reason: "Document expired".to_string(),
            can_resubmit: true,
            rejected_at: now(),
            occurred_at: now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: KycRejectedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
