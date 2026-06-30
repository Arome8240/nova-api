//! Integration test: publish and consume a `PaymentInitiatedEvent` end-to-end.
//!
//! Requires Docker. Spin up a Redpanda (Kafka-compatible) container via
//! testcontainers, publish one event, consume it, verify round-trip equality.
//!
//! Run with:
//!   cargo test -p kova-kafka --test integration_test
//!
//! These tests are skipped in environments where Docker is unavailable — the
//! test binary exits cleanly when the container fails to start.

use std::time::Duration;

use futures::StreamExt;
use kova_kafka::{KovaKafkaConsumer, KovaKafkaProducer};
use kova_types::{
    entities::CurrencyCode,
    events::payment::{PaymentInitiatedEvent, SCHEMA_VERSION, TOPIC_PAYMENT_INITIATED},
    ids::{KovaAccountId, KovaPaymentId, KovaUserId},
};
use rust_decimal_macros::dec;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::kafka::Kafka;
use uuid::Uuid;

fn make_event() -> PaymentInitiatedEvent {
    PaymentInitiatedEvent {
        schema_version: SCHEMA_VERSION,
        event_id: KovaPaymentId::from(Uuid::now_v7()),
        payment_id: KovaPaymentId::from(Uuid::now_v7()),
        user_id: KovaUserId::from(Uuid::now_v7()),
        source_account_id: KovaAccountId::from(Uuid::now_v7()),
        destination_account_id: KovaAccountId::from(Uuid::now_v7()),
        amount: dec!(1250.0000),
        currency: CurrencyCode::NGN,
        rail: "NIBSS".to_string(),
        idempotency_key: "test-key-001".to_string(),
        occurred_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
    }
}

#[tokio::test]
async fn publish_and_consume_payment_initiated_event() {
    // Start a Redpanda container (Kafka-compatible, much lighter than Kafka+ZK).
    let container = match Kafka::default().start().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Docker unavailable, skipping integration test: {e}");
            return;
        }
    };

    let port = container.get_host_port_ipv4(9092).await.unwrap();
    let brokers = format!("127.0.0.1:{port}");

    let original = make_event();
    let topic = TOPIC_PAYMENT_INITIATED;
    let key = original.payment_id.to_string();

    // ── Publish ───────────────────────────────────────────────────────────────
    let producer = KovaKafkaProducer::new(&brokers)
        .expect("producer construction failed");
    producer
        .publish(topic, &key, &original)
        .await
        .expect("publish failed");

    // ── Consume ───────────────────────────────────────────────────────────────
    let consumer = KovaKafkaConsumer::new(&brokers, "test-group", &[topic])
        .expect("consumer construction failed");

    let received = tokio::time::timeout(Duration::from_secs(15), async {
        let mut stream = consumer.consume::<PaymentInitiatedEvent>();
        stream.next().await
    })
    .await
    .expect("timed out waiting for message")
    .expect("stream ended before message")
    .expect("deserialization failed");

    let (event, raw_msg) = received;

    // Commit only after processing (manual commit pattern).
    consumer
        .commit_offset(&raw_msg)
        .expect("offset commit failed");

    // ── Assert round-trip equality ────────────────────────────────────────────
    assert_eq!(event.payment_id, original.payment_id);
    assert_eq!(event.user_id, original.user_id);
    assert_eq!(event.amount, original.amount);
    assert_eq!(event.currency, original.currency);
    assert_eq!(event.rail, original.rail);
    assert_eq!(event.schema_version, 1);
    assert_eq!(event.occurred_at, original.occurred_at);
}
