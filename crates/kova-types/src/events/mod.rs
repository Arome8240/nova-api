//! Kafka event schema for all KOVA services (Deliverable 4, TASK-021–027).
//!
//! All events follow these invariants:
//!   - `schema_version: u8 = 1` — consumers gate on this for schema evolution.
//!   - `occurred_at: DateTime<Utc>` — ISO 8601 string in JSON, never Unix epoch.
//!   - `Decimal` fields serialized as strings via `rust_decimal::serde::str`.
//!   - No `#[serde(deny_unknown_fields)]` — forward-compatible with new fields.

pub mod account;
pub mod card;
pub mod fraud;
pub mod kyc;
pub mod ledger;
pub mod notification;
pub mod payment;
