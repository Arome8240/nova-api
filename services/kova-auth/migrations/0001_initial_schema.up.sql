-- kova_auth_db — initial schema
-- Owns: user credentials, sessions, OTP codes, registered devices.
-- No cross-database references; foreign keys are intra-database only.

CREATE TABLE users (
    id              BINARY(16)   NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    phone_number    VARCHAR(20)  NOT NULL,
    pin_hash        VARCHAR(255) NOT NULL,
    kyc_status      ENUM('Unverified','Pending','UnderReview','Approved','Rejected','Expired')
                                 NOT NULL DEFAULT 'Unverified',
    created_at      DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at      DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    PRIMARY KEY (id),
    UNIQUE KEY uq_users_phone_number (phone_number)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Refresh-token sessions. revoked_at IS NULL means the session is live.
CREATE TABLE sessions (
    id                   BINARY(16)   NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    user_id              BINARY(16)   NOT NULL,
    refresh_token_hash   VARCHAR(255) NOT NULL,
    device_fingerprint   VARCHAR(255) NOT NULL,
    expires_at           DATETIME     NOT NULL,
    revoked_at           DATETIME     NULL,
    created_at           DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,

    PRIMARY KEY (id),
    -- Fast lookup by refresh token on every authenticated request.
    INDEX idx_sessions_refresh_token_hash (refresh_token_hash),
    INDEX idx_sessions_user_id            (user_id),
    CONSTRAINT fk_sessions_user_id
        FOREIGN KEY (user_id) REFERENCES users (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- One-time passwords (argon2id-hashed). used_at IS NULL means unused.
CREATE TABLE otp_codes (
    id          BINARY(16)   NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    user_id     BINARY(16)   NOT NULL,
    code_hash   VARCHAR(255) NOT NULL,
    expires_at  DATETIME     NOT NULL,
    used_at     DATETIME     NULL,
    created_at  DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,

    PRIMARY KEY (id),
    INDEX idx_otp_codes_user_id (user_id),
    CONSTRAINT fk_otp_codes_user_id
        FOREIGN KEY (user_id) REFERENCES users (id),
    -- OTP must expire after it was created.
    CONSTRAINT chk_otp_codes_expires
        CHECK (expires_at > created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Registered devices for push notifications and fraud signals.
CREATE TABLE devices (
    id                  BINARY(16)   NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    user_id             BINARY(16)   NOT NULL,
    device_fingerprint  VARCHAR(255) NOT NULL,
    device_name         VARCHAR(255) NULL,
    push_token          VARCHAR(500) NULL,
    last_seen_at        DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
    registered_at       DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,

    PRIMARY KEY (id),
    -- One registration per (user, device) pair.
    UNIQUE KEY uq_devices_user_fingerprint (user_id, device_fingerprint),
    CONSTRAINT fk_devices_user_id
        FOREIGN KEY (user_id) REFERENCES users (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
