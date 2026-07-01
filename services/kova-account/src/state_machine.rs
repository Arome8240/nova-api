//! Account status state machine (TASK-046).
//!
//! Valid transitions:
//!
//! ```text
//!   PendingKYC ──KYC approved──→ Active
//!   Active     ──freeze─────────→ Frozen
//!   Active     ──close──────────→ Closed   (balance must be 0, checked before calling)
//!   Frozen     ──unfreeze───────→ Active
//!   Frozen     ──close──────────→ Closed
//!   Closed     ──*──────────────→ ✗  (terminal — no exit from Closed)
//! ```
//!
//! All other transitions return `Err(InvalidTransitionError)`.

use kova_types::{
    entities::AccountStatus,
    errors::InvalidTransitionError,
};

/// Apply a status transition, returning the new status on success.
///
/// The caller is responsible for persisting the new status to the database
/// before treating the transition as committed.
pub fn transition(
    current: AccountStatus,
    target: AccountStatus,
) -> Result<AccountStatus, InvalidTransitionError> {
    use AccountStatus::*;

    let valid = matches!(
        (current, target),
        (PendingKYC, Active)
            | (Active, Frozen)
            | (Active, Closed)
            | (Frozen, Active)
            | (Frozen, Closed)
    );

    if valid {
        Ok(target)
    } else {
        Err(InvalidTransitionError::new(
            format!("{current:?}"),
            format!("{target:?}"),
        ))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kova_types::entities::AccountStatus::*;

    fn ok(from: AccountStatus, to: AccountStatus) {
        assert!(
            transition(from, to).is_ok(),
            "{from:?} → {to:?} should be valid"
        );
    }

    fn err(from: AccountStatus, to: AccountStatus) {
        assert!(
            transition(from, to).is_err(),
            "{from:?} → {to:?} should be invalid"
        );
    }

    #[test]
    fn valid_transitions() {
        ok(PendingKYC, Active);
        ok(Active, Frozen);
        ok(Active, Closed);
        ok(Frozen, Active);
        ok(Frozen, Closed);
    }

    #[test]
    fn closed_is_terminal() {
        err(Closed, Active);
        err(Closed, Frozen);
        err(Closed, PendingKYC);
        err(Closed, Closed);
    }

    #[test]
    fn pending_kyc_cannot_go_to_frozen_or_closed() {
        err(PendingKYC, Frozen);
        err(PendingKYC, Closed);
        err(PendingKYC, PendingKYC);
    }

    #[test]
    fn active_cannot_go_to_pending_kyc() {
        err(Active, PendingKYC);
        err(Active, Active);
    }

    #[test]
    fn frozen_cannot_go_to_pending_kyc() {
        err(Frozen, PendingKYC);
        err(Frozen, Frozen);
    }

    #[test]
    fn transition_error_has_correct_message() {
        let e = transition(Closed, Active).unwrap_err();
        assert_eq!(e.to_string(), "invalid transition from 'Closed' to 'Active'");
    }
}
