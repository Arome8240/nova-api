-- kova_account_db — initial schema
-- Owns: accounts and their lifecycle state.
-- IMPORTANT: No `balance` column exists. Balance is ALWAYS derived from kova-ledger.
-- Any future migration adding a balance column must be rejected at code review.

CREATE TABLE accounts (
    id              BINARY(16)    NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    user_id         BINARY(16)    NOT NULL,
    account_number  VARCHAR(20)   NOT NULL,
    currency        ENUM('NGN','GBP','USD','KES')
                                  NOT NULL,
    status          ENUM('Active','Frozen','Closed','PendingKYC')
                                  NOT NULL DEFAULT 'PendingKYC',
    created_at      DATETIME      NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at      DATETIME      NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    closed_at       DATETIME      NULL,

    -- Computed numeric suffix of account_number for efficient range queries.
    -- Example: 'KOVA0000000042' → 42. Stored so it can be indexed.
    account_number_idx BIGINT UNSIGNED
        GENERATED ALWAYS AS (CAST(SUBSTRING(account_number, 5) AS UNSIGNED)) STORED,

    PRIMARY KEY (id),
    -- Account number is globally unique.
    UNIQUE KEY uq_accounts_account_number (account_number),
    -- A user may hold at most one account per currency.
    UNIQUE KEY uq_accounts_user_currency (user_id, currency),
    -- Index the computed suffix for ordered pagination.
    INDEX idx_accounts_number_idx (account_number_idx),
    INDEX idx_accounts_user_id    (user_id),

    -- closed_at may only be set when the account is Closed.
    CONSTRAINT chk_accounts_closed_at
        CHECK (closed_at IS NULL OR status = 'Closed'),
    -- Account number must be "KOVA" followed by exactly 10 digits.
    CONSTRAINT chk_accounts_account_number
        CHECK (account_number REGEXP '^KOVA[0-9]{10}$')
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
