-- kova_cards_db — initial schema
-- Owns: virtual/physical card records and card transaction authorisations.
-- SECURITY: Raw PANs must NEVER appear in this database. vault_token is an
-- HMAC-derived reference to the PAN stored in an external card vault.

CREATE TABLE cards (
    id              BINARY(16)      NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    account_id      BINARY(16)      NOT NULL,
    user_id         BINARY(16)      NOT NULL,
    -- External vault reference — NOT a PAN. Never log this value.
    vault_token     VARCHAR(255)    NOT NULL,
    -- Last four digits for display only (e.g. on receipts).
    last_four       CHAR(4)         NOT NULL,
    expiry_month    TINYINT UNSIGNED NOT NULL,
    expiry_year     SMALLINT UNSIGNED NOT NULL,
    status          ENUM('Active','Frozen','Cancelled')
                                    NOT NULL DEFAULT 'Active',
    -- JSON spend controls: per_tx_limit, daily_limit, blocked_mccs[], etc.
    spend_controls  JSON            NULL,
    created_at      DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at      DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    PRIMARY KEY (id),
    -- Each vault token maps to exactly one card record.
    UNIQUE KEY uq_cards_vault_token (vault_token),
    INDEX idx_cards_account_id (account_id),
    INDEX idx_cards_user_id    (user_id),

    -- Enforce 4-digit numeric string. Prevents accidental PAN storage.
    CONSTRAINT chk_cards_last_four
        CHECK (last_four REGEXP '^[0-9]{4}$'),
    CONSTRAINT chk_cards_expiry_month
        CHECK (expiry_month BETWEEN 1 AND 12)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Individual card authorisation and settlement events.
CREATE TABLE card_transactions (
    id              BINARY(16)      NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    card_id         BINARY(16)      NOT NULL,
    amount          DECIMAL(19,4)   NOT NULL,
    currency        ENUM('NGN','GBP','USD','KES')
                                    NOT NULL,
    merchant_name   VARCHAR(255)    NOT NULL,
    -- ISO 18245 Merchant Category Code.
    mcc             VARCHAR(10)     NULL,
    status          ENUM('Authorized','Settled','Declined','Reversed')
                                    NOT NULL,
    authorized_at   DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP,
    settled_at      DATETIME        NULL,

    PRIMARY KEY (id),
    -- Pagination: most recent transactions per card first.
    INDEX idx_card_txns_card_authorized (card_id, authorized_at DESC),
    -- Full-text search for receipt matching and merchant analytics.
    FULLTEXT INDEX ft_card_txns_merchant_name (merchant_name),
    CONSTRAINT fk_card_txns_card_id
        FOREIGN KEY (card_id) REFERENCES cards (id),
    CONSTRAINT chk_card_txns_amount
        CHECK (amount > 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
