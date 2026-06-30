//! KYC status and risk level types shared across kova-kyc, kova-account, and kova-payments.
//!
//! `KycStatus` is the single source of truth for which statuses unlock payment and card
//! capabilities. No other service should re-implement this logic.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

// ── KycStatus ─────────────────────────────────────────────────────────────────

/// The verification lifecycle state of a KOVA user's KYC application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KycStatus {
    /// No KYC application has been submitted.
    Unverified,
    /// Application submitted, awaiting review.
    Pending,
    /// A compliance officer is actively reviewing the application.
    UnderReview,
    /// KYC passed — full account capabilities unlocked.
    Approved,
    /// KYC rejected — user must resubmit.
    Rejected,
    /// KYC approval has lapsed; user must renew.
    Expired,
}

impl_sqlx_string_enum!(KycStatus,
    Unverified  => "Unverified",
    Pending     => "Pending",
    UnderReview => "UnderReview",
    Approved    => "Approved",
    Rejected    => "Rejected",
    Expired     => "Expired",
);

impl KycStatus {
    /// Returns `true` if this status permits initiating a payment.
    pub fn allows_payment(&self) -> bool {
        matches!(self, Self::Approved)
    }

    /// Returns `true` if this status permits issuing a new card.
    pub fn allows_card_issuance(&self) -> bool {
        matches!(self, Self::Approved)
    }

    /// Returns `true` if transitioning to `next` is a valid KYC workflow step.
    ///
    /// Valid transitions:
    /// - `Unverified → Pending` (user submits)
    /// - `Pending → UnderReview` (reviewer picks up)
    /// - `Pending → Rejected` (quick rejection without full review)
    /// - `UnderReview → Approved`
    /// - `UnderReview → Rejected`
    /// - `Approved → Expired` (time passes, needs renewal)
    /// - `Rejected → Pending` (user resubmits after rejection)
    /// - `Expired → Pending` (user renews after expiry)
    pub fn can_transition_to(&self, next: KycStatus) -> bool {
        use KycStatus::*;
        matches!(
            (self, next),
            (Unverified, Pending)
                | (Pending, UnderReview)
                | (Pending, Rejected)
                | (UnderReview, Approved)
                | (UnderReview, Rejected)
                | (Approved, Expired)
                | (Rejected, Pending)
                | (Expired, Pending)
        )
    }
}

impl TryFrom<&str> for KycStatus {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_str(s)
    }
}

// ── KycRiskLevel ──────────────────────────────────────────────────────────────

/// The risk tier assigned to a user's KYC application by the compliance system.
///
/// Callers receiving `Unknown` from `kova-kyc` must treat it as `High`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KycRiskLevel {
    Low,
    Medium,
    High,
}

impl_sqlx_string_enum!(KycRiskLevel,
    Low    => "Low",
    Medium => "Medium",
    High   => "High",
);

impl TryFrom<&str> for KycRiskLevel {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_str(s)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_allows_payment() {
        assert!(KycStatus::Approved.allows_payment());
    }

    #[test]
    fn unverified_blocks_payment() {
        assert!(!KycStatus::Unverified.allows_payment());
    }

    #[test]
    fn all_non_approved_block_payment() {
        for status in [
            KycStatus::Pending,
            KycStatus::UnderReview,
            KycStatus::Rejected,
            KycStatus::Expired,
        ] {
            assert!(!status.allows_payment(), "{status} should block payment");
        }
    }

    #[test]
    fn approved_allows_card_issuance() {
        assert!(KycStatus::Approved.allows_card_issuance());
    }

    #[test]
    fn approved_to_expired_is_valid() {
        assert!(KycStatus::Approved.can_transition_to(KycStatus::Expired));
    }

    #[test]
    fn rejected_to_approved_is_invalid() {
        assert!(!KycStatus::Rejected.can_transition_to(KycStatus::Approved));
    }

    #[test]
    fn valid_transitions() {
        assert!(KycStatus::Unverified.can_transition_to(KycStatus::Pending));
        assert!(KycStatus::Pending.can_transition_to(KycStatus::UnderReview));
        assert!(KycStatus::Pending.can_transition_to(KycStatus::Rejected));
        assert!(KycStatus::UnderReview.can_transition_to(KycStatus::Approved));
        assert!(KycStatus::UnderReview.can_transition_to(KycStatus::Rejected));
        assert!(KycStatus::Rejected.can_transition_to(KycStatus::Pending));
        assert!(KycStatus::Expired.can_transition_to(KycStatus::Pending));
    }

    #[test]
    fn invalid_transitions() {
        assert!(!KycStatus::Approved.can_transition_to(KycStatus::Pending));
        assert!(!KycStatus::Approved.can_transition_to(KycStatus::Rejected));
        assert!(!KycStatus::Unverified.can_transition_to(KycStatus::Approved));
        assert!(!KycStatus::Rejected.can_transition_to(KycStatus::Expired));
    }

    #[test]
    fn display_matches_mysql_enum_value() {
        assert_eq!(KycStatus::UnderReview.to_string(), "UnderReview");
        assert_eq!(KycRiskLevel::Medium.to_string(), "Medium");
    }

    #[test]
    fn try_from_str_valid() {
        assert_eq!(KycStatus::try_from("Approved"), Ok(KycStatus::Approved));
        assert_eq!(KycRiskLevel::try_from("High"), Ok(KycRiskLevel::High));
    }

    #[test]
    fn try_from_str_invalid() {
        assert!(KycStatus::try_from("approved").is_err()); // case-sensitive
        assert!(KycStatus::try_from("Unknown").is_err());
        assert!(KycRiskLevel::try_from("Critical").is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let status = KycStatus::Approved;
        let json = serde_json::to_string(&status).unwrap();
        let back: KycStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }
}
