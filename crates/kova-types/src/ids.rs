//! Newtype ID wrappers for all KOVA domain entities.
//!
//! Every ID wraps a [`uuid::Uuid`] (generated as UUIDv7 for insert-order
//! locality on InnoDB B-tree indexes) and is stored in MySQL as `BINARY(16)`.
//!
//! # MySQL storage convention
//!
//! Insert:  `UUID_TO_BIN(?, true)`  — swap flag preserves time-ordering.
//! Select:  `BIN_TO_UUID(id, true)` — must match the insert flag.
//!
//! sqlx handles the raw 16-byte encoding automatically when the `uuid` feature
//! is enabled. The SQL helper functions are used in hand-written queries only.
//!
//! # Type safety
//!
//! Each newtype is a distinct Rust type. Passing a `KovaUserId` where a
//! `KovaAccountId` is expected is a **compile error** — the compiler rejects
//! it before any code runs.
//!
//! ```compile_fail
//! # use kova_types::ids::{KovaUserId, KovaAccountId};
//! fn takes_account(id: KovaAccountId) {}
//! let user_id = KovaUserId::new();
//! takes_account(user_id); // error[E0308]: mismatched types
//! ```

use serde::{Deserialize, Serialize};
use sqlx::MySql;
use uuid::Uuid;

/// Generate a newtype ID wrapper around [`uuid::Uuid`].
///
/// Provides:
/// - `new()` — generates a fresh UUIDv7
/// - `From<Uuid>` / `Into<Uuid>`
/// - `Display` — hyphenated UUID string
/// - `sqlx::Type`, `sqlx::Encode`, `sqlx::Decode` for MySQL `BINARY(16)`
/// - `serde::Serialize` / `serde::Deserialize` as a hyphenated UUID string
macro_rules! define_kova_id {
    ($(#[$attr:meta])* $name:ident) => {
        $(#[$attr])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a new ID using UUIDv7 (time-ordered).
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Return the inner [`Uuid`].
            pub fn into_uuid(self) -> Uuid {
                self.0
            }

            /// View the inner [`Uuid`] by reference.
            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Return the ID as raw bytes (16 bytes, big-endian).
            /// Matches the byte layout of MySQL `BINARY(16)`.
            pub fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Uuid {
                id.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        // ── sqlx MySQL integration ────────────────────────────────────────

        impl sqlx::Type<MySql> for $name {
            fn type_info() -> sqlx::mysql::MySqlTypeInfo {
                <Uuid as sqlx::Type<MySql>>::type_info()
            }

            fn compatible(ty: &sqlx::mysql::MySqlTypeInfo) -> bool {
                <Uuid as sqlx::Type<MySql>>::compatible(ty)
            }
        }

        impl<'r> sqlx::Decode<'r, MySql> for $name {
            fn decode(
                value: <MySql as sqlx::Database>::ValueRef<'r>,
            ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
                let uuid = <Uuid as sqlx::Decode<'r, MySql>>::decode(value)?;
                Ok(Self(uuid))
            }
        }

        impl<'q> sqlx::Encode<'q, MySql> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut <MySql as sqlx::Database>::ArgumentBuffer<'q>,
            ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
                <Uuid as sqlx::Encode<'q, MySql>>::encode_by_ref(&self.0, buf)
            }
        }
    };
}

// ── ID types ─────────────────────────────────────────────────────────────────

define_kova_id!(
    /// Unique identifier for a KOVA user (maps to `kova_auth_db.users.id`).
    KovaUserId
);

define_kova_id!(
    /// Unique identifier for a KOVA account (maps to `kova_account_db.accounts.id`).
    KovaAccountId
);

define_kova_id!(
    /// Unique identifier for a KOVA payment (maps to `kova_payments_db.payments.id`).
    KovaPaymentId
);

define_kova_id!(
    /// Unique identifier for a KOVA transaction / ledger entry
    /// (maps to `kova_ledger_db.ledger_entries.id`).
    KovaTransactionId
);

define_kova_id!(
    /// Unique identifier for a KOVA ledger entry
    /// (maps to `kova_ledger_db.ledger_entries.id`).
    KovaLedgerEntryId
);

define_kova_id!(
    /// Unique identifier for a KOVA card (maps to `kova_cards_db.cards.id`).
    KovaCardId
);

define_kova_id!(
    /// Unique identifier for a KOVA KYC document
    /// (maps to `kova_kyc_db.kyc_documents.id`).
    KovaKycDocumentId
);

define_kova_id!(
    /// Unique identifier for a KOVA audit event
    /// (maps to `kova_audit_db.audit_events.id`).
    KovaAuditEventId
);

define_kova_id!(
    /// Unique identifier for a KOVA FX quote
    /// (stored as a UUIDv7 string in Redis, referenced in `kova_fx_db`).
    KovaFxQuoteId
);

define_kova_id!(
    /// Unique identifier for a KOVA in-app notification
    /// (maps to `kova_notifications_db.notifications.id`).
    KovaNotificationId
);

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_generates_distinct_ids() {
        let a = KovaUserId::new();
        let b = KovaUserId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn roundtrip_uuid_conversion() {
        let uuid = Uuid::now_v7();
        let id = KovaAccountId::from(uuid);
        let back: Uuid = id.into();
        assert_eq!(uuid, back);
    }

    #[test]
    fn display_is_hyphenated_uuid() {
        let uuid = Uuid::now_v7();
        let id = KovaPaymentId::from(uuid);
        // Hyphenated format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx (36 chars)
        let s = id.to_string();
        assert_eq!(s.len(), 36);
        assert_eq!(s.chars().filter(|&c| c == '-').count(), 4);
    }

    #[test]
    fn serde_roundtrip_as_uuid_string() {
        let id = KovaUserId::new();
        let json = serde_json::to_string(&id).unwrap();
        // Must be a quoted hyphenated UUID, not a nested object
        assert!(json.starts_with('"'));
        assert!(json.ends_with('"'));
        assert_eq!(json.len(), 38); // 36 chars + 2 quotes

        let decoded: KovaUserId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, decoded);
    }

    #[test]
    fn as_bytes_returns_16_bytes() {
        let id = KovaLedgerEntryId::new();
        assert_eq!(id.as_bytes().len(), 16);
    }

    #[test]
    fn different_id_types_are_distinct() {
        // Compile-time proof: the two types are distinct via their TypeId at runtime.
        use std::any::TypeId;
        assert_ne!(
            TypeId::of::<KovaUserId>(),
            TypeId::of::<KovaAccountId>(),
        );
        assert_ne!(
            TypeId::of::<KovaPaymentId>(),
            TypeId::of::<KovaLedgerEntryId>(),
        );
    }
}
