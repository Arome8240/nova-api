//! Payment state machine — `PaymentStatus` and `PaymentEvent`.
//!
//! ## State diagram
//!
//! ```text
//! Initiated ──ValidationPassed──► Validating ──FraudApproved──► FraudCheck
//!     │                                │                             │   │
//!  ValidationFailed                 FraudBlocked              FraudBlocked  SubmittedToRail (same-currency)
//!     │                                │                             │         │
//!     ▼                                ▼                             ▼         ▼
//!   Failed ◄────────────────────── Failed ◄── FraudBlocked  FxConversion   SettlementSubmitted
//!                                                                    │             │   │
//!                                                               FxConverted   RailSettled  RailFailed
//!                                                                    │             │         │
//!                                                                    ▼             ▼         ▼
//!                                                          SettlementSubmitted   Settled   Failed
//!                                                                                   │
//!                                                                          RefundRequested
//!                                                                                   │
//!                                                                               Refunded
//! ```
//!
//! Terminal states: `Settled`, `Failed`, `Refunded`, `Expired`.

use serde::{Deserialize, Serialize};

use crate::errors::InvalidTransitionError;

// ── PaymentStatus ─────────────────────────────────────────────────────────────

/// The lifecycle state of a KOVA payment.
///
/// The `Display` impl returns the exact snake_case string stored in the MySQL
/// `payments.status` ENUM column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaymentStatus {
    /// Payment created; awaiting input validation.
    Initiated,
    /// Input validation passed; awaiting fraud decision.
    Validating,
    /// Fraud evaluation in progress.
    FraudCheck,
    /// FX rate locked; currency conversion in progress.
    FxConversion,
    /// Submitted to the payment rail; awaiting settlement.
    SettlementSubmitted,
    /// Rail confirmed settlement — terminal success.
    Settled,
    /// Payment failed at any stage — terminal failure.
    Failed,
    /// Settled payment has been refunded — terminal.
    Refunded,
    /// Payment timed out — terminal.
    Expired,
}

impl_sqlx_string_enum!(PaymentStatus,
    Initiated           => "initiated",
    Validating          => "validating",
    FraudCheck          => "fraud_check",
    FxConversion        => "fx_conversion",
    SettlementSubmitted => "settlement_submitted",
    Settled             => "settled",
    Failed              => "failed",
    Refunded            => "refunded",
    Expired             => "expired",
);

impl PaymentStatus {
    /// Returns `true` if this is a terminal state (no further transitions possible).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Settled | Self::Failed | Self::Refunded | Self::Expired)
    }
}

// ── PaymentEvent ──────────────────────────────────────────────────────────────

/// An event that drives the [`PaymentStatus`] state machine forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaymentEvent {
    /// Basic input validation passed.
    ValidationPassed,
    /// Basic input validation failed (e.g., invalid account, amount zero).
    ValidationFailed,
    /// Fraud engine approved the payment.
    FraudApproved,
    /// Fraud engine blocked the payment.
    FraudBlocked,
    /// Fraud engine flagged for manual review (payment stays in FraudCheck).
    FraudReview,
    /// FX rate locked and currency converted.
    FxConverted,
    /// Payment submitted to the payment rail (same-currency shortcut from FraudCheck).
    SubmittedToRail,
    /// Payment rail confirmed settlement.
    RailSettled,
    /// Payment rail reported failure.
    RailFailed,
    /// Refund was requested for a settled payment.
    RefundRequested,
    /// Payment timed out (any non-terminal state).
    Expired,
}

impl std::fmt::Display for PaymentEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::ValidationPassed => "ValidationPassed",
            Self::ValidationFailed => "ValidationFailed",
            Self::FraudApproved => "FraudApproved",
            Self::FraudBlocked => "FraudBlocked",
            Self::FraudReview => "FraudReview",
            Self::FxConverted => "FxConverted",
            Self::SubmittedToRail => "SubmittedToRail",
            Self::RailSettled => "RailSettled",
            Self::RailFailed => "RailFailed",
            Self::RefundRequested => "RefundRequested",
            Self::Expired => "Expired",
        };
        f.write_str(s)
    }
}

// ── State machine ─────────────────────────────────────────────────────────────

impl PaymentStatus {
    /// Attempt to transition this status forward by applying `event`.
    ///
    /// Returns the new status on success, or [`InvalidTransitionError`] if
    /// the `(status, event)` pair is not in the allowed transition graph.
    pub fn transition(self, event: PaymentEvent) -> Result<PaymentStatus, InvalidTransitionError> {
        use PaymentEvent as E;
        use PaymentStatus as S;

        let next = match (self, event) {
            // ── Initiated ────────────────────────────────────────────────
            (S::Initiated, E::ValidationPassed) => S::Validating,
            (S::Initiated, E::ValidationFailed) => S::Failed,
            (S::Initiated, E::Expired) => S::Expired,

            // ── Validating ───────────────────────────────────────────────
            (S::Validating, E::FraudApproved) => S::FraudCheck,
            (S::Validating, E::FraudBlocked) => S::Failed,
            (S::Validating, E::Expired) => S::Expired,

            // ── FraudCheck ───────────────────────────────────────────────
            // FraudReview leaves the payment in FraudCheck for manual review.
            (S::FraudCheck, E::FraudReview) => S::FraudCheck,
            (S::FraudCheck, E::FraudApproved) => S::FxConversion,
            (S::FraudCheck, E::FraudBlocked) => S::Failed,
            // Same-currency payment: skip FX, go straight to rail.
            (S::FraudCheck, E::SubmittedToRail) => S::SettlementSubmitted,
            (S::FraudCheck, E::Expired) => S::Expired,

            // ── FxConversion ─────────────────────────────────────────────
            (S::FxConversion, E::FxConverted) => S::SettlementSubmitted,
            (S::FxConversion, E::Expired) => S::Expired,

            // ── SettlementSubmitted ───────────────────────────────────────
            (S::SettlementSubmitted, E::RailSettled) => S::Settled,
            (S::SettlementSubmitted, E::RailFailed) => S::Failed,

            // ── Settled ───────────────────────────────────────────────────
            (S::Settled, E::RefundRequested) => S::Refunded,

            // All other combinations are invalid.
            _ => {
                return Err(InvalidTransitionError::new(
                    self.to_string(),
                    event.to_string(),
                ))
            }
        };
        Ok(next)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use PaymentEvent as E;
    use PaymentStatus as S;

    fn ok(from: S, ev: E, to: S) {
        assert_eq!(
            from.transition(ev),
            Ok(to),
            "{from} + {ev} should → {to}"
        );
    }

    fn err(from: S, ev: E) {
        assert!(
            from.transition(ev).is_err(),
            "{from} + {ev} should be invalid"
        );
    }

    // ── Valid transitions (16) ────────────────────────────────────────────────

    #[test]
    fn valid_initiated_validation_passed() { ok(S::Initiated, E::ValidationPassed, S::Validating); }
    #[test]
    fn valid_initiated_validation_failed() { ok(S::Initiated, E::ValidationFailed, S::Failed); }
    #[test]
    fn valid_initiated_expired() { ok(S::Initiated, E::Expired, S::Expired); }
    #[test]
    fn valid_validating_fraud_approved() { ok(S::Validating, E::FraudApproved, S::FraudCheck); }
    #[test]
    fn valid_validating_fraud_blocked() { ok(S::Validating, E::FraudBlocked, S::Failed); }
    #[test]
    fn valid_validating_expired() { ok(S::Validating, E::Expired, S::Expired); }
    #[test]
    fn valid_fraud_check_review_stays() { ok(S::FraudCheck, E::FraudReview, S::FraudCheck); }
    #[test]
    fn valid_fraud_check_approved_to_fx() { ok(S::FraudCheck, E::FraudApproved, S::FxConversion); }
    #[test]
    fn valid_fraud_check_blocked() { ok(S::FraudCheck, E::FraudBlocked, S::Failed); }
    #[test]
    fn valid_fraud_check_same_currency() { ok(S::FraudCheck, E::SubmittedToRail, S::SettlementSubmitted); }
    #[test]
    fn valid_fraud_check_expired() { ok(S::FraudCheck, E::Expired, S::Expired); }
    #[test]
    fn valid_fx_conversion_converted() { ok(S::FxConversion, E::FxConverted, S::SettlementSubmitted); }
    #[test]
    fn valid_fx_conversion_expired() { ok(S::FxConversion, E::Expired, S::Expired); }
    #[test]
    fn valid_settlement_submitted_rail_settled() { ok(S::SettlementSubmitted, E::RailSettled, S::Settled); }
    #[test]
    fn valid_settlement_submitted_rail_failed() { ok(S::SettlementSubmitted, E::RailFailed, S::Failed); }
    #[test]
    fn valid_settled_refund_requested() { ok(S::Settled, E::RefundRequested, S::Refunded); }

    // ── Invalid transitions (17) ──────────────────────────────────────────────

    #[test]
    fn invalid_settled_to_initiated() { err(S::Settled, E::ValidationPassed); }
    #[test]
    fn invalid_settled_fraud_approved() { err(S::Settled, E::FraudApproved); }
    #[test]
    fn invalid_refunded_refund_again() { err(S::Refunded, E::RefundRequested); }
    #[test]
    fn invalid_failed_rail_settled() { err(S::Failed, E::RailSettled); }
    #[test]
    fn invalid_expired_validation() { err(S::Expired, E::ValidationPassed); }
    #[test]
    fn invalid_initiated_rail_settled() { err(S::Initiated, E::RailSettled); }
    #[test]
    fn invalid_initiated_refund() { err(S::Initiated, E::RefundRequested); }
    #[test]
    fn invalid_settled_submitted_to_rail() { err(S::Settled, E::SubmittedToRail); }
    #[test]
    fn invalid_refunded_rail_settled() { err(S::Refunded, E::RailSettled); }
    #[test]
    fn invalid_failed_validation_passed() { err(S::Failed, E::ValidationPassed); }
    #[test]
    fn invalid_expired_rail_settled() { err(S::Expired, E::RailSettled); }
    #[test]
    fn invalid_validating_rail_settled() { err(S::Validating, E::RailSettled); }
    #[test]
    fn invalid_fx_validation_passed() { err(S::FxConversion, E::ValidationPassed); }
    #[test]
    fn invalid_settlement_fraud_approved() { err(S::SettlementSubmitted, E::FraudApproved); }
    #[test]
    fn invalid_initiated_fx_converted() { err(S::Initiated, E::FxConverted); }
    #[test]
    fn invalid_settlement_submitted_expired() { err(S::SettlementSubmitted, E::Expired); }
    #[test]
    fn invalid_fraud_check_fx_converted() { err(S::FraudCheck, E::FxConverted); }

    // ── Misc ─────────────────────────────────────────────────────────────────

    #[test]
    fn terminal_states() {
        assert!(S::Settled.is_terminal());
        assert!(S::Failed.is_terminal());
        assert!(S::Refunded.is_terminal());
        assert!(S::Expired.is_terminal());
        assert!(!S::Initiated.is_terminal());
        assert!(!S::Validating.is_terminal());
    }

    #[test]
    fn display_matches_mysql_enum_strings() {
        assert_eq!(S::FraudCheck.to_string(), "fraud_check");
        assert_eq!(S::FxConversion.to_string(), "fx_conversion");
        assert_eq!(S::SettlementSubmitted.to_string(), "settlement_submitted");
        assert_eq!(S::Initiated.to_string(), "initiated");
    }

    #[test]
    fn serde_roundtrip() {
        let s = S::SettlementSubmitted;
        let json = serde_json::to_string(&s).unwrap();
        let back: S = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
