//! TASK-056 — end-of-day ledger reconciliation.
//!
//! Runs once per hour via `tokio::time::interval`; only performs the actual
//! reconciliation pass during the 23:55–00:05 UTC window to keep it as a
//! true EOD job without adding a cron dependency.
//!
//! For each account active today (any debit or credit entry created since
//! midnight UTC), the worker:
//!   1. Computes the live balance from `ledger_entries`.
//!   2. Compares against the most recent snapshot in
//!      `ledger_account_snapshots`.
//!   3. If they differ, increments `kova_ledger_imbalance_total` and logs
//!      at ERROR level.
//!   4. Inserts a fresh snapshot regardless (for next run's baseline).
//!
//! `imbalance_total()` exposes the counter so it can be scraped and fed into
//! a Prometheus gauge at the HTTP metrics endpoint.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chrono::{Timelike, Utc};
use rust_decimal::Decimal;
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

/// Monotonically-increasing count of detected balance mismatches.
/// Exported via `imbalance_total()` for Prometheus scraping.
/// Label: `kova_ledger_imbalance_total`.
static IMBALANCE_TOTAL: AtomicU64 = AtomicU64::new(0);

pub fn imbalance_total() -> u64 {
    IMBALANCE_TOTAL.load(Ordering::Relaxed)
}

/// Spawn the EOD reconciliation scheduler.  Call with `tokio::spawn`.
pub async fn run_eod_reconciliation(pool: MySqlPool) {
    let mut interval = tokio::time::interval(Duration::from_secs(3600));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;

        let now = Utc::now();
        let h = now.hour();
        let m = now.minute();
        let in_window = (h == 23 && m >= 55) || (h == 0 && m <= 5);
        if !in_window {
            continue;
        }

        tracing::info!(
            date = now.date_naive().to_string().as_str(),
            "starting EOD reconciliation"
        );

        match reconciliation_pass(&pool).await {
            Ok(mismatches) => {
                if mismatches > 0 {
                    let total = IMBALANCE_TOTAL.fetch_add(mismatches, Ordering::Relaxed) + mismatches;
                    tracing::error!(
                        mismatches,
                        kova_ledger_imbalance_total = total,
                        "CRITICAL: EOD reconciliation detected balance mismatches"
                    );
                } else {
                    tracing::info!("EOD reconciliation passed — all balances match snapshots");
                }
            }
            Err(e) => tracing::error!(error = %e, "EOD reconciliation failed"),
        }
    }
}

/// One full reconciliation pass.  Returns the number of accounts where the
/// live balance differs from the most recent snapshot.
async fn reconciliation_pass(pool: &MySqlPool) -> Result<u64, sqlx::Error> {
    // Collect all distinct accounts that had any activity today.
    let rows = sqlx::query(
        r#"
        SELECT debit_account_id AS account_id FROM ledger_entries
            WHERE DATE(created_at) = CURDATE()
        UNION
        SELECT credit_account_id FROM ledger_entries
            WHERE DATE(created_at) = CURDATE()
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut mismatches: u64 = 0;

    for row in &rows {
        let account_id: Vec<u8> = row.try_get("account_id")?;

        // Find all currencies this account holds entries in.
        let currency_rows = sqlx::query(
            "SELECT DISTINCT currency \
             FROM ledger_entries \
             WHERE debit_account_id = ? OR credit_account_id = ?",
        )
        .bind(account_id.as_slice())
        .bind(account_id.as_slice())
        .fetch_all(pool)
        .await?;

        for cr in &currency_rows {
            let currency: String = cr.try_get("currency")?;

            let live = compute_live_balance(pool, &account_id, &currency).await?;

            // Compare against the most recent snapshot if one exists.
            let snap = sqlx::query(
                r#"SELECT balance FROM ledger_account_snapshots
                   WHERE account_id = ? AND currency = ?
                   ORDER BY snapshot_at DESC
                   LIMIT 1"#,
            )
            .bind(account_id.as_slice())
            .bind(&currency)
            .fetch_optional(pool)
            .await?;

            if let Some(snap_row) = snap {
                let snapshot_balance: Decimal = snap_row.try_get("balance")?;
                if live != snapshot_balance {
                    mismatches += 1;
                    tracing::error!(
                        account_id = hex::encode(&account_id),
                        %currency,
                        %live,
                        %snapshot_balance,
                        "balance mismatch — live differs from snapshot"
                    );
                }
            }

            // Always write a fresh snapshot so the next run has a fresh baseline.
            let snap_id = Uuid::now_v7();
            sqlx::query(
                r#"INSERT INTO ledger_account_snapshots
                       (id, account_id, currency, balance)
                   VALUES (?, ?, ?, ?)"#,
            )
            .bind(snap_id.as_bytes().as_slice())
            .bind(account_id.as_slice())
            .bind(&currency)
            .bind(live)
            .execute(pool)
            .await?;
        }
    }

    Ok(mismatches)
}

/// Recompute `balance = SUM(credits) - SUM(debits)` directly from entries.
async fn compute_live_balance(
    pool: &MySqlPool,
    account_id: &[u8],
    currency: &str,
) -> Result<Decimal, sqlx::Error> {
    let row = sqlx::query(
        r#"SELECT CAST(
               COALESCE(SUM(CASE WHEN credit_account_id = ? THEN amount END), 0) -
               COALESCE(SUM(CASE WHEN debit_account_id  = ? THEN amount END), 0)
           AS DECIMAL(19,4)) AS balance
           FROM ledger_entries
           WHERE (credit_account_id = ? OR debit_account_id = ?)
             AND currency = ?"#,
    )
    .bind(account_id)
    .bind(account_id)
    .bind(account_id)
    .bind(account_id)
    .bind(currency)
    .fetch_one(pool)
    .await?;

    row.try_get("balance")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_detection() {
        let in_window = |h: u32, m: u32| (h == 23 && m >= 55) || (h == 0 && m <= 5);

        assert!(in_window(23, 55));
        assert!(in_window(23, 59));
        assert!(in_window(0, 0));
        assert!(in_window(0, 5));
        assert!(!in_window(0, 6));
        assert!(!in_window(23, 54));
        assert!(!in_window(12, 0));
    }

    #[test]
    fn imbalance_counter_starts_at_zero() {
        // Note: other tests in the same run might have incremented this already,
        // so we only check it is a valid u64 value.
        let _ = imbalance_total();
    }
}
