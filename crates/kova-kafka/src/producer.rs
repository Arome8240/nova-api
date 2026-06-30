//! `KovaKafkaProducer` — thin wrapper over `rdkafka::FutureProducer`.
//!
//! Key design points:
//!   - `acks=all` is enforced at construction; callers cannot override it.
//!   - `publish` retries up to `MAX_RETRIES` times on transient errors with
//!     exponential back-off (50 ms → 100 ms → 200 ms). Permanent errors
//!     (`RDKafkaErrorCode::MessageTimedOut` is transient; `UnknownTopic` is not)
//!     propagate immediately.
//!   - Never call `publish` from inside a MySQL transaction. Use the outbox pattern.

use std::time::Duration;

use rdkafka::{
    error::RDKafkaErrorCode,
    producer::{FutureProducer, FutureRecord},
    ClientConfig,
};
use serde::Serialize;
use tracing::{debug, warn};

use crate::error::KafkaError;

const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF_MS: u64 = 50;

pub struct KovaKafkaProducer {
    inner: FutureProducer,
}

impl KovaKafkaProducer {
    /// Build a producer from broker addresses (comma-separated).
    ///
    /// `acks=all` and `enable.idempotence=true` are always set regardless of
    /// what the caller passes in `extra_config`.
    pub fn new(brokers: &str) -> Result<Self, KafkaError> {
        let inner = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            // Durability: all in-sync replicas must acknowledge the write.
            .set("acks", "all")
            // Idempotent producer prevents duplicates on retry at the broker level.
            .set("enable.idempotence", "true")
            // Internal retries inside rdkafka before we see an error.
            .set("message.send.max.retries", "5")
            // Linger allows micro-batching without sacrificing per-message latency much.
            .set("linger.ms", "5")
            .set("compression.type", "snappy")
            .create::<FutureProducer>()?;

        Ok(Self { inner })
    }

    /// Serialize `event` as JSON and publish it to `topic` with `key` as the
    /// Kafka message key (use the entity UUID so partitioning keeps all events
    /// for one entity on the same partition).
    ///
    /// Retries up to `MAX_RETRIES` times on transient errors.
    pub async fn publish<E: Serialize>(
        &self,
        topic: &str,
        key: &str,
        event: &E,
    ) -> Result<(), KafkaError> {
        let payload = serde_json::to_vec(event)?;

        let mut last_err = String::new();
        for attempt in 1..=MAX_RETRIES {
            let record = FutureRecord::to(topic)
                .key(key)
                .payload(payload.as_slice());

            match self.inner.send(record, Duration::from_secs(5)).await {
                Ok(delivery) => {
                    debug!(
                        topic,
                        key,
                        partition = delivery.partition,
                        offset = delivery.offset,
                        "kafka message published"
                    );
                    return Ok(());
                }
                Err((err, _owned_msg)) => {
                    if is_transient(&err) {
                        let backoff = BASE_BACKOFF_MS * (1 << (attempt - 1));
                        warn!(
                            topic,
                            key,
                            attempt,
                            backoff_ms = backoff,
                            error = %err,
                            "transient kafka publish error, retrying"
                        );
                        last_err = err.to_string();
                        tokio::time::sleep(Duration::from_millis(backoff)).await;
                    } else {
                        return Err(KafkaError::Rdkafka(err));
                    }
                }
            }
        }

        Err(KafkaError::RetriesExhausted {
            attempts: MAX_RETRIES,
            last_error: last_err,
        })
    }
}

/// True for errors where a retry has a reasonable chance of succeeding.
fn is_transient(err: &rdkafka::error::KafkaError) -> bool {
    use rdkafka::error::KafkaError as KErr;
    match err {
        KErr::MessageProduction(code) => matches!(
            code,
            RDKafkaErrorCode::MessageTimedOut
                | RDKafkaErrorCode::QueueFull
                | RDKafkaErrorCode::BrokerTransportFailure
                | RDKafkaErrorCode::NetworkException
                | RDKafkaErrorCode::RequestTimedOut
        ),
        // Any other KafkaError variant is considered permanent.
        _ => false,
    }
}
