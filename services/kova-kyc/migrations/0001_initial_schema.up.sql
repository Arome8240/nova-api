-- kova_kyc_db — initial schema
-- Owns: KYC applications and uploaded identity documents.
-- s3_object_key stores the S3 object key only (NOT a full URL).
-- Presigned URLs are generated at request time to avoid URL expiry in stored data.

CREATE TABLE kyc_applications (
    id                  BINARY(16)      NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    user_id             BINARY(16)      NOT NULL,
    -- Status values must match KycStatus::to_string() in kova-types exactly.
    status              ENUM('Unverified','Pending','UnderReview','Approved','Rejected','Expired')
                                        NOT NULL DEFAULT 'Unverified',
    risk_level          ENUM('Low','Medium','High')
                                        NULL,
    submitted_at        DATETIME        NULL,
    reviewed_at         DATETIME        NULL,
    expires_at          DATETIME        NULL,
    rejection_reason    TEXT            NULL,
    reviewer_id         BINARY(16)      NULL,
    created_at          DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at          DATETIME        NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,

    PRIMARY KEY (id),
    -- One active KYC application per user.
    UNIQUE KEY uq_kyc_applications_user_id (user_id),
    -- Expiry cron queries this index to find records requiring renewal notices.
    INDEX idx_kyc_applications_expires_at (expires_at),
    INDEX idx_kyc_applications_status     (status),

    -- reviewed_at may only be set once a final decision has been made.
    CONSTRAINT chk_kyc_reviewed
        CHECK (reviewed_at IS NULL OR status IN ('Approved', 'Rejected'))
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Identity documents uploaded as part of a KYC application.
CREATE TABLE kyc_documents (
    id               BINARY(16)    NOT NULL DEFAULT (UUID_TO_BIN(UUID(), true)),
    application_id   BINARY(16)    NOT NULL,
    document_type    ENUM(
                         'passport',
                         'national_id',
                         'drivers_license',
                         'utility_bill',
                         'bank_statement'
                     ) NOT NULL,
    -- S3 object key only — NOT a full URL or presigned URL.
    s3_object_key    VARCHAR(500)  NOT NULL,
    -- OCR extraction results populated asynchronously; NULL until processed.
    ocr_result       JSON          NULL,
    uploaded_at      DATETIME      NOT NULL DEFAULT CURRENT_TIMESTAMP,

    PRIMARY KEY (id),
    INDEX idx_kyc_docs_application_id (application_id),
    CONSTRAINT fk_kyc_docs_application_id
        FOREIGN KEY (application_id) REFERENCES kyc_applications (id),
    -- Object key must be a non-empty string.
    CONSTRAINT chk_kyc_docs_s3_key
        CHECK (CHAR_LENGTH(s3_object_key) > 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
