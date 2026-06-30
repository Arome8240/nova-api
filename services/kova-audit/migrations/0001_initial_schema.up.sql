-- kova_audit_db — initial schema
-- Owns: immutable append-only audit event log for all KOVA services.
-- The kova_audit MySQL user is granted SELECT, INSERT only (no UPDATE, no DELETE).
-- BEFORE UPDATE and BEFORE DELETE triggers provide a second layer of defence
-- against mutations via direct DBA access.
--
-- HASH CHAIN INTEGRITY:
-- Each audit event includes a SHA-256 hash chained to the previous event:
--   event_hash = SHA-256(previous_hash || created_at || event_type || payload)
-- The chain is computed and written by the kova-audit service at insert time.
-- Verification: re-compute each event_hash and compare; a mismatch indicates
-- tampering. Note: the chain provides tamper EVIDENCE, not cryptographic proof —
-- a MySQL superuser could disable the trigger and alter rows. For regulatory
-- WORM compliance, export to an immutable storage layer (e.g. AWS S3 Object Lock).

CREATE TABLE audit_events (
    id              BINARY(16)      NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    event_type      VARCHAR(100)    NOT NULL,
    entity_type     VARCHAR(50)     NOT NULL,
    entity_id       BINARY(16)      NOT NULL,
    actor_id        BINARY(16)      NULL,
    payload         JSON            NOT NULL,
    -- SHA-256 hash (32 bytes) of the previous audit event in the chain.
    -- NULL only for the very first event in the chain.
    previous_hash   BINARY(32)      NULL,
    -- SHA-256 hash of this event's canonical serialisation.
    event_hash      BINARY(32)      NULL,
    -- DATETIME(6): microsecond precision for correct ordering of the hash chain
    -- when multiple events are inserted within the same second.
    created_at      DATETIME(6)     NOT NULL DEFAULT CURRENT_TIMESTAMP(6),

    PRIMARY KEY (id),
    -- Filtered audit queries: "all events for payment X" or "all events for user Y".
    INDEX idx_audit_entity      (entity_type, entity_id, created_at),
    -- Actor queries: "all actions taken by user Z".
    INDEX idx_audit_actor       (actor_id, created_at),
    INDEX idx_audit_event_type  (event_type, created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Immutability trigger — UPDATE.
-- The MySQL user has no UPDATE privilege, but this trigger fires even for
-- root-level DBA updates performed outside the service's connection pool.
CREATE TRIGGER trg_audit_events_no_update
BEFORE UPDATE ON audit_events
FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'audit events are immutable';

-- Immutability trigger — DELETE.
CREATE TRIGGER trg_audit_events_no_delete
BEFORE DELETE ON audit_events
FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'audit events cannot be deleted';
