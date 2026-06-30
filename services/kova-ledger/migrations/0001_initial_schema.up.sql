-- kova_ledger_db — initial schema
-- Owns: double-entry ledger postings and account balance snapshots.
-- ledger_entries are IMMUTABLE — enforced by BEFORE UPDATE and BEFORE DELETE triggers.
-- The kova_ledger MySQL user is granted SELECT, INSERT only (no UPDATE, no DELETE).

CREATE TABLE ledger_entries (
    id                  BINARY(16)      NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    debit_account_id    BINARY(16)      NOT NULL,
    credit_account_id   BINARY(16)      NOT NULL,
    -- Amount is always positive; direction is determined by the debit/credit account sides.
    amount              DECIMAL(19,4)   NOT NULL,
    currency            ENUM('NGN','GBP','USD','KES')
                                        NOT NULL,
    -- Unique per business operation; prevents duplicate postings.
    idempotency_key     VARCHAR(255)    NOT NULL,
    payment_id          BINARY(16)      NULL,
    -- DATETIME(6): microsecond precision is required for correct ordering when
    -- multiple entries are created within the same second.
    created_at          DATETIME(6)     NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    metadata            JSON            NULL,

    PRIMARY KEY (id),
    -- Idempotency: any duplicate submission for the same business event is rejected.
    UNIQUE KEY uq_ledger_idempotency_key (idempotency_key),
    -- Balance queries scan by account and time range.
    INDEX idx_ledger_debit_account  (debit_account_id,  created_at),
    INDEX idx_ledger_credit_account (credit_account_id, created_at),
    INDEX idx_ledger_payment_id     (payment_id),

    -- Amount must be strictly positive — zero and negative are programming errors.
    CONSTRAINT chk_ledger_amount_positive
        CHECK (amount > 0),
    -- Self-transfers are accounting errors that would silently cancel out.
    CONSTRAINT chk_ledger_no_self_transfer
        CHECK (debit_account_id != credit_account_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Block all UPDATEs. The MySQL user has no UPDATE privilege, but the trigger
-- provides a second layer of defence against accidental mutations via DBA access.
CREATE TRIGGER trg_ledger_entries_no_update
BEFORE UPDATE ON ledger_entries
FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'ledger entries are immutable';

-- Block all DELETEs for the same reason.
CREATE TRIGGER trg_ledger_entries_no_delete
BEFORE DELETE ON ledger_entries
FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'ledger entries cannot be deleted';

-- Periodic balance snapshots used by kova-reconciliation.
-- A snapshot represents the sum of all ledger entries for an account up to snapshot_at.
CREATE TABLE ledger_account_snapshots (
    id              BINARY(16)      NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    account_id      BINARY(16)      NOT NULL,
    currency        ENUM('NGN','GBP','USD','KES')
                                    NOT NULL,
    -- Computed balance at the time of this snapshot.
    balance         DECIMAL(19,4)   NOT NULL,
    snapshot_at     DATETIME(6)     NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    -- The ledger entry id that was the latest entry when this snapshot was taken.
    last_entry_id   BINARY(16)      NULL,

    PRIMARY KEY (id),
    INDEX idx_snapshots_account (account_id, snapshot_at DESC)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
