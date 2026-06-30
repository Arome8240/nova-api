use thiserror::Error;

#[derive(Debug, Error)]
pub enum KafkaError {
    /// rdkafka producer / consumer error.
    #[error("kafka error: {0}")]
    Rdkafka(#[from] rdkafka::error::KafkaError),

    /// JSON serialization failed (bug in the caller — event struct is not serializable).
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),

    /// All producer retries exhausted.
    #[error("publish failed after {attempts} attempts: {last_error}")]
    RetriesExhausted {
        attempts: u32,
        last_error: String,
    },

    /// Consumer received a message whose payload could not be deserialized.
    #[error("deserialization error on topic {topic}: {source}")]
    Deserialize {
        topic: String,
        #[source]
        source: serde_json::Error,
    },

    /// Consumer offset commit failed.
    #[error("offset commit failed: {0}")]
    CommitFailed(rdkafka::error::KafkaError),
}
