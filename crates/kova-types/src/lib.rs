//! `kova-types` — shared domain types for all KOVA services.
//!
//! All service crates add `kova-types = { path = "../../crates/kova-types" }`
//! to access these types.
//!
//! # Modules
//!
//! - [`ids`]      — Newtype UUIDv7 wrappers (TASK-007)
//! - [`money`]    — `Money<C>` phantom-typed struct (TASK-008)
//! - [`entities`] — Core entity structs with `sqlx::FromRow` (TASK-009)
//! - [`payment`]  — `PaymentStatus` state machine (TASK-010)
//! - [`kyc`]      — `KycStatus` and `KycRiskLevel` (TASK-011)
//! - [`errors`]   — `KovaError` hierarchy (TASK-012)
//! - [`events`]   — Kafka event schema for all services (TASK-021–027)

// Pull macros into scope for all child modules BEFORE the module declarations.
#[macro_use]
mod macros;

pub mod entities;
pub mod errors;
pub mod events;
pub mod ids;
pub mod kyc;
pub mod money;
pub mod payment;

// ── Top-level re-exports ──────────────────────────────────────────────────────

pub use entities::{Account, AccountStatus, AuditEvent, Card, CardStatus, CurrencyCode, FxQuote, LedgerEntry, User};

pub use errors::{DatabaseError, InvalidTransitionError, KovaError};

pub use ids::{
    KovaAccountId, KovaAuditEventId, KovaCardId, KovaFxQuoteId, KovaKycDocumentId,
    KovaLedgerEntryId, KovaNotificationId, KovaPaymentId, KovaTransactionId, KovaUserId,
};

pub use kyc::{KycRiskLevel, KycStatus};

pub use money::{Currency, Money, GBP, KES, NGN, USD};

pub use payment::{PaymentEvent, PaymentStatus};

// ── Event re-exports ─────────────────────────────────────────────────────────

pub use events::account::{
    AccountActivatedEvent, AccountClosedEvent, AccountCreatedEvent, AccountLimitChangedEvent,
    AccountSuspendedEvent,
};
pub use events::card::{
    CardActivatedEvent, CardBlockedEvent, CardExpiredEvent, CardIssuedEvent,
    CardTransactionApprovedEvent, CardTransactionDeclinedEvent, CardUnblockedEvent, DeclineReason,
};
pub use events::fraud::{FraudCaseCreatedEvent, FraudDecision, FraudDecisionEvent};
pub use events::kyc::{
    KycApprovedEvent, KycDocumentUploadedEvent, KycRejectedEvent, KycReviewStartedEvent,
    KycSubmittedEvent,
};
pub use events::ledger::{LedgerEntryCreatedEvent, LedgerReconciliationEvent};
pub use events::notification::{InAppNotificationEvent, NotificationType, PushNotificationRequestedEvent};
pub use events::payment::{
    PaymentExpiredEvent, PaymentFailedEvent, PaymentFraudCheckedEvent, PaymentFxConvertedEvent,
    PaymentInitiatedEvent, PaymentRefundedEvent, PaymentSettledEvent, PaymentValidatedEvent,
};
