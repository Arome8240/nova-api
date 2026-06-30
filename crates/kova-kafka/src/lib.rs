//! `kova-kafka` — Kafka producer and consumer infrastructure for KOVA services.
//!
//! # Usage
//!
//! ## Producer (outbox worker)
//! ```rust,ignore
//! use kova_kafka::KovaKafkaProducer;
//! use kova_types::events::payment::{PaymentInitiatedEvent, TOPIC_PAYMENT_INITIATED};
//!
//! let producer = KovaKafkaProducer::new("localhost:9092")?;
//! producer.publish(TOPIC_PAYMENT_INITIATED, &event.payment_id.to_string(), &event).await?;
//! ```
//!
//! ## Consumer
//! ```rust,ignore
//! use kova_kafka::KovaKafkaConsumer;
//! use kova_types::events::payment::PaymentInitiatedEvent;
//! use futures::StreamExt;
//!
//! let consumer = KovaKafkaConsumer::new("localhost:9092", "kova-ledger", &["kova.payment.initiated"])?;
//! let mut stream = consumer.consume::<PaymentInitiatedEvent>();
//! while let Some(result) = stream.next().await {
//!     let (event, raw) = result?;
//!     // process event ...
//!     consumer.commit_offset(&raw)?;
//! }
//! ```

pub mod consumer;
pub mod error;
pub mod producer;

pub use consumer::KovaKafkaConsumer;
pub use error::KafkaError;
pub use producer::KovaKafkaProducer;
