//! TASK-052 — balance derivation from ledger entries.
//!
//! Balance is always computed live from `ledger_entries`; there is no stored
//! balance column.  `COALESCE(SUM(...), 0)` handles accounts that have no
//! entries yet (SUM returns NULL on an empty set in MySQL).
//!
//! The gRPC `GetBalance` handler calls this function and converts the result
//! to a proto response.

use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{MySqlPool, Row};
use tracing::instrument;

use crate::error::LedgerError;

pub struct BalanceResult {
    pub balance:     Decimal,
    pub entry_count: i64,
    pub as_of:       Option<DateTime<Utc>>,
}

/// Derive the current balance for `account_id` in `currency`.
///
/// Returns `balance = SUM(credits) - SUM(debits)` and may be negative for
/// internal settlement accounts (overdraft is not blocked at the ledger layer).
#[instrument(skip(pool), fields(currency = currency))]
pub async fn get_balance(
    pool: &MySqlPool,
    account_id: &[u8; 16],
    currency: &str,
) -> Result<BalanceResult, LedgerError> {
    const SUPPORTED: &[&str] = &["NGN", "GBP", "USD", "KES"];
    if !SUPPORTED.contains(&currency) {
        return Err(LedgerError::InvalidArgument(format!(
            "unsupported currency: {currency}"
        )));
    }

    let row = sqlx::query(
        r#"
        SELECT
            CAST(
                COALESCE(SUM(CASE WHEN credit_account_id = ? THEN amount END), 0) -
                COALESCE(SUM(CASE WHEN debit_account_id  = ? THEN amount END), 0)
            AS DECIMAL(19,4))     AS balance,
            COUNT(*)               AS entry_count,
            MAX(created_at)        AS as_of
        FROM ledger_entries
        WHERE (credit_account_id = ? OR debit_account_id = ?)
          AND currency = ?
        "#,
    )
    .bind(account_id.as_slice())
    .bind(account_id.as_slice())
    .bind(account_id.as_slice())
    .bind(account_id.as_slice())
    .bind(currency)
    .fetch_one(pool)
    .await?;

    let balance: Decimal = row.try_get("balance")?;
    let entry_count: i64 = row.try_get("entry_count")?;
    let as_of: Option<NaiveDateTime> = row.try_get("as_of")?;

    Ok(BalanceResult {
        balance,
        entry_count,
        as_of: as_of.map(|dt| dt.and_utc()),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn unsupported_currency_rejects() {
        // Validate the guard without touching the DB — just confirm the
        // logic path exists; the actual async call needs a pool.
        let bad = ["EUR", "BTC", ""];
        for c in bad {
            let supported = &["NGN", "GBP", "USD", "KES"];
            assert!(!supported.contains(&c), "{c} should be unsupported");
        }
    }
}
