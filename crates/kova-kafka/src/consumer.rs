//! `KovaKafkaConsumer` — typed streaming consumer.
//!
//! Key design points:
//!   - `consume<E>()` returns a `Stream<Item = Result<(E, OwnedMessage)>>`.
//!     The caller drives the stream; only AFTER successful processing should
//!     the caller invoke `commit_offset` to advance the committed offset.
//!   - `enable.auto.commit = false` is enforced so that a crash between
//!     deserialization and handling does not silently skip a message.
//!   - One `KovaKafkaConsumer` instance owns one Kafka consumer group member.

use rdkafka::{
    consumer::{CommitMode, Consumer, StreamConsumer},
    message::OwnedMessage,
    ClientConfig, Message,
};
use serde::de::DeserializeOwned;
use tracing::{debug, warn};

use futures::StreamExt;

use crate::error::KafkaError;

pub struct KovaKafkaConsumer {
    inner: StreamConsumer,
}

impl KovaKafkaConsumer {
    /// Create a consumer and subscribe to `topics`.
    ///
    /// `group_id` identifies the consumer group; use the service name
    /// (e.g. `"kova-ledger"`).
    pub fn new(
        brokers: &str,
        group_id: &str,
        topics: &[&str],
    ) -> Result<Self, KafkaError> {
        let inner = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("group.id", group_id)
            // Manual offset commit: commit only after successful processing.
            .set("enable.auto.commit", "false")
            // Read from the beginning if no committed offset exists (new groups).
            .set("auto.offset.reset", "earliest")
            .set("session.timeout.ms", "10000")
            .set("fetch.min.bytes", "1")
            .create::<StreamConsumer>()?;

        inner.subscribe(topics)?;
        Ok(Self { inner })
    }

    /// Return an infinite async stream of deserialized events paired with their
    /// raw `OwnedMessage` (needed for `commit_offset`).
    ///
    /// Deserialization errors are surfaced as `Err(KafkaError::Deserialize)`.
    /// The caller decides whether to DLQ or skip.
    pub fn consume<E: DeserializeOwned + 'static>(
        &self,
    ) -> impl futures::Stream<Item = Result<(E, OwnedMessage), KafkaError>> + '_
    {
        self.inner.stream().map(|msg_result| {
            let msg = msg_result?;
            let topic = msg.topic().to_string();
            let payload = msg.payload().unwrap_or_default();

            debug!(topic, "received kafka message ({} bytes)", payload.len());

            let event: E = serde_json::from_slice(payload).map_err(|source| {
                warn!(topic, "kafka deserialization error: {source}");
                KafkaError::Deserialize { topic, source }
            })?;

            Ok((event, msg.detach()))
        })
    }

    /// Commit the offset for a processed message (store + sync commit).
    ///
    /// Uses `store_offset` + `commit(None, Sync)` because `commit_message`
    /// only accepts `&BorrowedMessage`, not `&OwnedMessage`.
    ///
    /// Call this only after the event has been durably processed.
    pub fn commit_offset(&self, msg: &OwnedMessage) -> Result<(), KafkaError> {
        // Store the offset for this specific (topic, partition, offset+1) position.
        self.inner
            .store_offset(msg.topic(), msg.partition(), msg.offset())
            .map_err(KafkaError::CommitFailed)?;

        // Flush stored offsets to the broker synchronously.
        self.inner
            .commit_consumer_state(CommitMode::Sync)
            .map_err(KafkaError::CommitFailed)
    }
}
