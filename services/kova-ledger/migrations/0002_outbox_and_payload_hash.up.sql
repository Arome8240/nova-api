-- TASK-053: add payload_hash to ledger_entries for idempotency mismatch detection.
-- SHA-256 hex digest of (debit_account_id, credit_account_id, amount, currency).
-- NULL allowed for entries created before this migration.
ALTER TABLE ledger_entries
    ADD COLUMN payload_hash CHAR(64) NULL AFTER idempotency_key;

-- TASK-055: outbox table for reliable Kafka publishing.
-- Written atomically in the same transaction as ledger_entries INSERT.
-- The outbox worker polls this table and marks rows published_at when delivered.
-- NOTE: the application MySQL user requires UPDATE privilege on this table;
--       the SELECT+INSERT-only restriction applies to ledger_entries exclusively.
CREATE TABLE ledger_outbox (
    id           BINARY(16)   NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    entry_id     BINARY(16)   NOT NULL,
    topic        VARCHAR(128) NOT NULL,
    payload_json JSON         NOT NULL,
    created_at   DATETIME(6)  NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    published_at DATETIME(6)  NULL,

    PRIMARY KEY (id),
    -- Outbox worker: unpublished rows ordered by creation time.
    INDEX idx_outbox_unpublished (published_at, created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
