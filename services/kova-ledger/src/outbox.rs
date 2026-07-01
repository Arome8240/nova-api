//! TASK-055 — transactional outbox worker.
//!
//! The outbox pattern guarantees at-least-once Kafka delivery without
//! distributed transactions: `ledger_entries` and `ledger_outbox` are written
//! atomically (same MySQL transaction), then this worker polls
//! `ledger_outbox` and publishes rows to Kafka before marking them done.
//!
//! `SELECT ... FOR UPDATE SKIP LOCKED` ensures multiple replicas of
//! kova-ledger can run the worker concurrently without duplicating work.
//!
//! Exponential backoff on errors prevents hot-looping against a broken
//! broker.  Back-off is capped at 30 s.
//!
//! NOTE: `ledger_outbox` requires UPDATE privilege (unlike `ledger_entries`
//! which is INSERT-only).  Grant `UPDATE ON kova_ledger_db.ledger_outbox` to
//! the application MySQL user.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use sqlx::{MySqlPool, Row};
use tracing::{debug, error};

use crate::error::LedgerError;

// ── Publisher trait ───────────────────────────────────────────────────────────

#[async_trait]
pub trait OutboxPublisher: Send + Sync {
    async fn publish(&self, topic: &str, key: &str, payload: &Value) -> Result<(), LedgerError>;
}

/// No-op publisher used when the Kafka feature is disabled.
pub struct NoOpPublisher;

#[async_trait]
impl OutboxPublisher for NoOpPublisher {
    async fn publish(&self, topic: &str, key: &str, _payload: &Value) -> Result<(), LedgerError> {
        debug!(topic, key, "outbox: no-op publish");
        Ok(())
    }
}

/// Kafka publisher backed by `kova-kafka`.  Compiled only with `--features kafka`.
#[cfg(feature = "kafka")]
pub struct KafkaPublisher(pub std::sync::Arc<kova_kafka::KovaKafkaProducer>);

#[cfg(feature = "kafka")]
#[async_trait]
impl OutboxPublisher for KafkaPublisher {
    async fn publish(&self, topic: &str, key: &str, payload: &Value) -> Result<(), LedgerError> {
        self.0
            .publish(topic, key, payload)
            .await
            .map_err(|e| LedgerError::Internal(e.to_string()))
    }
}

// ── Worker loop ───────────────────────────────────────────────────────────────

/// Spawn the outbox worker.  Call with `tokio::spawn`.
pub async fn run_outbox_worker(
    pool: MySqlPool,
    publisher: std::sync::Arc<dyn OutboxPublisher>,
) {
    const IDLE_SLEEP_MS: u64 = 100;
    const MAX_BACKOFF_SECS: u64 = 30;
    let mut consecutive_errors: u32 = 0;

    loop {
        match process_batch(&pool, publisher.as_ref()).await {
            Ok(0) => {
                // Nothing pending — sleep briefly before polling again.
                consecutive_errors = 0;
                tokio::time::sleep(Duration::from_millis(IDLE_SLEEP_MS)).await;
            }
            Ok(n) => {
                consecutive_errors = 0;
                debug!(n, "outbox batch published");
            }
            Err(e) => {
                consecutive_errors += 1;
                // Exponential backoff: 1s, 2s, 4s, … capped at MAX_BACKOFF_SECS
                let backoff = (1u64 << consecutive_errors.min(5)).min(MAX_BACKOFF_SECS);
                error!(
                    error = %e,
                    consecutive_errors,
                    backoff_secs = backoff,
                    "outbox worker error"
                );
                tokio::time::sleep(Duration::from_secs(backoff)).await;
            }
        }
    }
}

async fn process_batch(
    pool: &MySqlPool,
    publisher: &dyn OutboxPublisher,
) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
    let mut tx = pool.begin().await?;

    let rows = sqlx::query(
        r#"SELECT id, entry_id, topic, payload_json
           FROM ledger_outbox
           WHERE published_at IS NULL
           ORDER BY created_at ASC
           LIMIT 10
           FOR UPDATE SKIP LOCKED"#,
    )
    .fetch_all(&mut *tx)
    .await?;

    if rows.is_empty() {
        tx.rollback().await.ok();
        return Ok(0);
    }

    let mut count = 0u32;
    for row in &rows {
        let id: Vec<u8> = row.try_get("id")?;
        let entry_id_bytes: Vec<u8> = row.try_get("entry_id")?;
        let topic: String = row.try_get("topic")?;
        let payload_json: serde_json::Value = row.try_get("payload_json")?;

        let key = hex::encode(&entry_id_bytes);

        publisher.publish(&topic, &key, &payload_json).await?;

        sqlx::query(
            "UPDATE ledger_outbox SET published_at = NOW(6) WHERE id = ?",
        )
        .bind(id.as_slice())
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_op_publisher_succeeds() {
        let p = NoOpPublisher;
        let result = p.publish("test.topic", "key1", &serde_json::json!({})).await;
        assert!(result.is_ok());
    }
}
