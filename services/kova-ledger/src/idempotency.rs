//! TASK-053 — idempotency helpers.
//!
//! `compute_payload_hash` produces a SHA-256 hex digest over the four
//! immutable fields of a posting (debit account, credit account, amount,
//! currency).  This hash is stored in `ledger_entries.payload_hash` and
//! compared on re-submission to distinguish a genuine idempotent retry
//! (same key, same payload → OK) from a programming error (same key,
//! different payload → ALREADY_EXISTS / IdempotencyMismatch).
//!
//! `cleanup_stale_idempotency_keys` is provided for completeness but is a
//! no-op: `ledger_entries` rows are immutable by schema design (DELETE
//! trigger + restricted DB user) and serve as the permanent audit trail.
//! Callers that want time-bounded idempotency should maintain a separate
//! mutable store.

use ring::digest;
use rust_decimal::Decimal;

/// SHA-256 hex digest of the canonical posting fields.
/// The hash does NOT include `idempotency_key`, `payment_id`, or `metadata`.
pub fn compute_payload_hash(
    debit_account_id: &[u8; 16],
    credit_account_id: &[u8; 16],
    amount: &Decimal,
    currency: &str,
) -> String {
    let mut ctx = digest::Context::new(&digest::SHA256);
    ctx.update(debit_account_id);
    ctx.update(credit_account_id);
    ctx.update(amount.to_string().as_bytes());
    ctx.update(currency.as_bytes());
    hex::encode(ctx.finish().as_ref())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    fn account_id(seed: u8) -> [u8; 16] {
        [seed; 16]
    }

    #[test]
    fn same_inputs_produce_same_hash() {
        let h1 = compute_payload_hash(&account_id(1), &account_id(2), &dec!(100.00), "NGN");
        let h2 = compute_payload_hash(&account_id(1), &account_id(2), &dec!(100.00), "NGN");
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_amount_different_hash() {
        let h1 = compute_payload_hash(&account_id(1), &account_id(2), &dec!(100.00), "NGN");
        let h2 = compute_payload_hash(&account_id(1), &account_id(2), &dec!(100.01), "NGN");
        assert_ne!(h1, h2);
    }

    #[test]
    fn different_currency_different_hash() {
        let h1 = compute_payload_hash(&account_id(1), &account_id(2), &dec!(100.00), "NGN");
        let h2 = compute_payload_hash(&account_id(1), &account_id(2), &dec!(100.00), "GBP");
        assert_ne!(h1, h2);
    }

    #[test]
    fn swapped_accounts_different_hash() {
        let h1 = compute_payload_hash(&account_id(1), &account_id(2), &dec!(100.00), "NGN");
        let h2 = compute_payload_hash(&account_id(2), &account_id(1), &dec!(100.00), "NGN");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_is_64_hex_chars() {
        let h = compute_payload_hash(&account_id(1), &account_id(2), &dec!(1.00), "USD");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
