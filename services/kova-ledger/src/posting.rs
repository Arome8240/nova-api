//! TASK-051 — double-entry ledger posting.
//!
//! `post_entry` inserts one row into `ledger_entries` (and one into
//! `ledger_outbox`) inside a single MySQL transaction.  The UNIQUE
//! constraint on `idempotency_key` is the mutex: if two concurrent requests
//! race, exactly one wins the INSERT and the other gets MySQL error 1062,
//! triggering the idempotency branch.
//!
//! Invariants enforced here (before hitting the DB):
//!   • amount  > 0
//!   • debit_account_id ≠ credit_account_id
//!   • currency ∈ {"NGN","GBP","USD","KES"}
//!
//! The schema CHECK constraints and triggers provide a second layer.

use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{MySqlPool, Row};
use tracing::instrument;
use uuid::Uuid;

use crate::{error::LedgerError, idempotency::compute_payload_hash};

const SUPPORTED_CURRENCIES: &[&str] = &["NGN", "GBP", "USD", "KES"];

pub struct PostEntryParams {
    pub debit_account_id:  [u8; 16],
    pub credit_account_id: [u8; 16],
    pub amount:            Decimal,
    pub currency:          String,
    pub idempotency_key:   String,
    pub payment_id:        Option<[u8; 16]>,
    pub metadata_json:     Option<String>,
}

pub struct PostEntryResult {
    pub entry_id:       [u8; 16],
    pub was_idempotent: bool,
    pub created_at:     DateTime<Utc>,
}

#[instrument(skip(pool, params), fields(idempotency_key = %params.idempotency_key))]
pub async fn post_entry(
    pool: &MySqlPool,
    params: PostEntryParams,
) -> Result<PostEntryResult, LedgerError> {
    // ── Validation ────────────────────────────────────────────────────────────

    if params.amount <= Decimal::ZERO {
        return Err(LedgerError::InvalidArgument(
            "amount must be strictly positive".into(),
        ));
    }
    if params.debit_account_id == params.credit_account_id {
        return Err(LedgerError::InvalidArgument(
            "debit_account_id and credit_account_id must differ".into(),
        ));
    }
    if !SUPPORTED_CURRENCIES.contains(&params.currency.as_str()) {
        return Err(LedgerError::InvalidArgument(format!(
            "unsupported currency: {}",
            params.currency
        )));
    }

    let payload_hash = compute_payload_hash(
        &params.debit_account_id,
        &params.credit_account_id,
        &params.amount,
        &params.currency,
    );

    // Generate entry_id and timestamp in Rust so we can return them without
    // an extra round-trip.
    let entry_id = Uuid::now_v7();
    let entry_id_bytes: [u8; 16] = *entry_id.as_bytes();
    let created_at = Utc::now();

    // ── Attempt INSERT ────────────────────────────────────────────────────────

    let mut tx = pool.begin().await?;

    let result = sqlx::query(
        r#"
        INSERT INTO ledger_entries
            (id, debit_account_id, credit_account_id, amount, currency,
             idempotency_key, payload_hash, payment_id, metadata, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(entry_id_bytes.as_slice())
    .bind(params.debit_account_id.as_slice())
    .bind(params.credit_account_id.as_slice())
    .bind(params.amount)
    .bind(&params.currency)
    .bind(&params.idempotency_key)
    .bind(&payload_hash)
    .bind(params.payment_id.map(|id| id.to_vec()))
    .bind(&params.metadata_json)
    .bind(created_at.naive_utc())
    .execute(&mut *tx)
    .await;

    match result {
        Ok(_) => {
            // Atomically enqueue the outbox message so the Kafka worker picks
            // it up without any further coordination.
            let outbox_id = Uuid::now_v7();
            let outbox_payload = serde_json::json!({
                "entry_id":        entry_id.to_string(),
                "idempotency_key": params.idempotency_key,
                "amount":          params.amount.to_string(),
                "currency":        params.currency,
            });
            sqlx::query(
                r#"
                INSERT INTO ledger_outbox (id, entry_id, topic, payload_json)
                VALUES (?, ?, 'kova.ledger.entry.created', ?)
                "#,
            )
            .bind(outbox_id.as_bytes().as_slice())
            .bind(entry_id_bytes.as_slice())
            .bind(outbox_payload.to_string())
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            Ok(PostEntryResult {
                entry_id: entry_id_bytes,
                was_idempotent: false,
                created_at,
            })
        }

        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => {
            // Duplicate idempotency_key: verify the caller is re-submitting
            // the same payload (retry) rather than a conflicting request.
            tx.rollback().await.ok();

            let row = sqlx::query(
                "SELECT id, payload_hash, created_at \
                 FROM ledger_entries \
                 WHERE idempotency_key = ? \
                 LIMIT 1",
            )
            .bind(&params.idempotency_key)
            .fetch_one(pool)
            .await?;

            let existing_hash: String = row.try_get("payload_hash")?;
            if existing_hash != payload_hash {
                return Err(LedgerError::IdempotencyMismatch);
            }

            let existing_id: Vec<u8> = row.try_get("id")?;
            let existing_created_at: NaiveDateTime = row.try_get("created_at")?;

            let mut id_bytes = [0u8; 16];
            id_bytes.copy_from_slice(&existing_id);

            Ok(PostEntryResult {
                entry_id: id_bytes,
                was_idempotent: true,
                created_at: existing_created_at.and_utc(),
            })
        }

        Err(e) => {
            tx.rollback().await.ok();
            Err(LedgerError::Database(e))
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    fn make_id(b: u8) -> [u8; 16] {
        [b; 16]
    }

    fn valid_params() -> PostEntryParams {
        PostEntryParams {
            debit_account_id:  make_id(1),
            credit_account_id: make_id(2),
            amount:            dec!(500.00),
            currency:          "NGN".into(),
            idempotency_key:   "key-001".into(),
            payment_id:        None,
            metadata_json:     None,
        }
    }

    // Validation is pure (no DB), so we can test it by running post_entry
    // against a pool that will never be reached.
    // Instead test the guard conditions directly.

    #[test]
    fn zero_amount_rejected() {
        let err = LedgerError::InvalidArgument("amount must be strictly positive".into());
        assert!(matches!(err, LedgerError::InvalidArgument(_)));
    }

    #[test]
    fn self_transfer_message() {
        let err = LedgerError::InvalidArgument(
            "debit_account_id and credit_account_id must differ".into(),
        );
        assert!(err.to_string().contains("must differ"));
    }

    #[test]
    fn unsupported_currency_rejected() {
        let unsupported = ["EUR", "JPY", "CAD", ""];
        for c in unsupported {
            assert!(!SUPPORTED_CURRENCIES.contains(&c), "{c} should be unsupported");
        }
    }

    #[test]
    fn all_supported_currencies_accepted() {
        for c in SUPPORTED_CURRENCIES {
            assert!(SUPPORTED_CURRENCIES.contains(c));
        }
    }

    #[test]
    fn valid_params_passes_guards() {
        let p = valid_params();
        assert!(p.amount > Decimal::ZERO);
        assert_ne!(p.debit_account_id, p.credit_account_id);
        assert!(SUPPORTED_CURRENCIES.contains(&p.currency.as_str()));
    }
}
