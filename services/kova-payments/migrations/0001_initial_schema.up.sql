-- kova_payments_db — initial schema
-- Owns: payment lifecycle state and the Kafka outbox for payment events.
-- The payment_outbox table implements the transactional outbox pattern:
-- Kafka messages are written here inside the same MySQL transaction as the
-- payments row, then published by a background worker that polls unpublished rows.
-- NEVER publish Kafka events directly inside a database transaction.

CREATE TABLE payments (
    id                      BINARY(16)      NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    user_id                 BINARY(16)      NOT NULL,
    source_account_id       BINARY(16)      NOT NULL,
    destination_account_id  BINARY(16)      NOT NULL,
    amount                  DECIMAL(19,4)   NOT NULL,
    currency                ENUM('NGN','GBP','USD','KES')
                                            NOT NULL,
    -- Status ENUM values must match PaymentStatus::to_string() in kova-types exactly.
    status                  ENUM(
                                'initiated',
                                'validating',
                                'fraud_check',
                                'fx_conversion',
                                'settlement_submitted',
                                'settled',
                                'failed',
                                'refunded',
                                'expired'
                            ) NOT NULL DEFAULT 'initiated',
    -- Payment rail identifier, e.g. 'NIBSS', 'FasterPayments', 'SEPA', 'internal'.
    rail                    VARCHAR(50)     NOT NULL,
    retry_count             INT             NOT NULL DEFAULT 0,
    next_retry_at           DATETIME        NULL,
    -- Client-supplied idempotency key; deduplicates concurrent submissions.
    idempotency_key         VARCHAR(255)    NOT NULL,
    created_at              DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at              DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    settled_at              DATETIME        NULL,
    failed_reason           TEXT            NULL,

    PRIMARY KEY (id),
    UNIQUE KEY uq_payments_idempotency_key (idempotency_key),
    INDEX idx_payments_user_id     (user_id, created_at DESC),
    INDEX idx_payments_status      (status, created_at DESC),
    INDEX idx_payments_source_acc  (source_account_id, created_at DESC),

    CONSTRAINT chk_payments_amount_positive
        CHECK (amount > 0),
    CONSTRAINT chk_payments_retry_count
        CHECK (retry_count >= 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Transactional outbox for Kafka event publishing.
-- The background worker queries: WHERE is_unpublished = 1 ORDER BY created_at
CREATE TABLE payment_outbox (
    id            BINARY(16)    NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    payment_id    BINARY(16)    NOT NULL,
    event_type    VARCHAR(100)  NOT NULL,
    payload       JSON          NOT NULL,
    -- NULL = pending publication; non-NULL = published timestamp.
    published_at  DATETIME      NULL,
    created_at    DATETIME      NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Generated column: 1 when unpublished, NULL when published.
    -- MySQL does not support partial indexes; indexing this generated column
    -- achieves the same effect — only rows with value 1 (unpublished) appear
    -- in the index, making the outbox poll query O(unpublished) not O(total).
    is_unpublished TINYINT
        GENERATED ALWAYS AS (IF(published_at IS NULL, 1, NULL)) VIRTUAL,

    PRIMARY KEY (id),
    -- The outbox worker polls this index; it will be small in steady state.
    INDEX idx_outbox_is_unpublished (is_unpublished, created_at),
    INDEX idx_outbox_payment_id     (payment_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
