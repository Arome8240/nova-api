-- Extend sessions for efficient revocation checking and refresh-token lookup.
--
-- token_selector: SHA-256(refresh_token_raw) as 64-char hex — indexed for O(1)
--   lookup on token refresh without scanning all sessions.
-- access_token_jti: JTI of the current access token — used by the auth
--   middleware to check session revocation without a full-table scan.
ALTER TABLE sessions
    ADD COLUMN token_selector      VARCHAR(64)  NULL AFTER refresh_token_hash,
    ADD COLUMN access_token_jti    VARCHAR(36)  NULL AFTER token_selector,
    ADD INDEX  idx_sessions_token_selector   (token_selector),
    ADD INDEX  idx_sessions_access_token_jti (access_token_jti);
