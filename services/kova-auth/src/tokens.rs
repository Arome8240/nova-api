//! JWT issuance and refresh-token rotation.
//!
//! Access tokens:  RS256, TTL 900 s — claims: sub, jti, iat, exp, iss, kova_device_id.
//! Refresh tokens: 32 random bytes (hex), TTL 30 days, stored using the
//!                 split-token pattern:
//!   - `token_selector` = SHA-256(raw) as 64-char hex — indexed, O(1) lookup.
//!   - `refresh_token_hash` = bcrypt(raw, cost=10) — constant-time verification.
//!
//! Rotation security: presenting a token whose `revoked_at IS NOT NULL` triggers
//! immediate revocation of ALL sessions for that user (token-theft signal).

use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

use kova_types::KovaUserId;

use crate::{crypto::random_hex_token, error::AuthError};

pub const ACCESS_TTL_SECS: i64 = 900;
pub const REFRESH_TTL_SECS: i64 = 2_592_000; // 30 days
const BCRYPT_COST: u32 = 10;

#[derive(Debug, Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KovaClaims {
    pub sub: String,
    pub exp: u64,
    pub iat: u64,
    pub jti: String,
    pub iss: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kova_device_id: Option<String>,
}

fn sha256_hex(data: &str) -> String {
    let d = digest(&SHA256, data.as_bytes());
    hex::encode(d.as_ref())
}

/// Sign an RS256 access token. Returns `(token_string, jti)`.
pub fn sign_access_token(
    user_id: KovaUserId,
    device_fingerprint: &str,
    key: &EncodingKey,
) -> Result<(String, String), AuthError> {
    let now = Utc::now().timestamp();
    let jti = Uuid::now_v7().to_string();
    let claims = KovaClaims {
        sub: user_id.to_string(),
        exp: (now + ACCESS_TTL_SECS) as u64,
        iat: now as u64,
        jti: jti.clone(),
        iss: "kova-auth".to_string(),
        kova_device_id: Some(device_fingerprint.to_string()),
    };
    let token = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, key)
        .map_err(|e| AuthError::Crypto(e.to_string()))?;
    Ok((token, jti))
}

/// Issue a fresh token pair and persist the session row.
///
/// The refresh token raw value is returned once; only its bcrypt hash + SHA-256
/// selector are stored in the database.
pub async fn issue_token_pair(
    user_id: KovaUserId,
    device_fingerprint: &str,
    key: &EncodingKey,
    db: &MySqlPool,
) -> Result<TokenPair, AuthError> {
    let (access_token, jti) = sign_access_token(user_id, device_fingerprint, key)?;

    let refresh_raw = random_hex_token()?;
    let selector = sha256_hex(&refresh_raw);
    let refresh_hash = bcrypt::hash(&refresh_raw, BCRYPT_COST)
        .map_err(|e| AuthError::Crypto(e.to_string()))?;

    let session_id = Uuid::now_v7().to_string();
    let expires_at = Utc::now() + Duration::seconds(REFRESH_TTL_SECS);

    sqlx::query(
        r#"
        INSERT INTO sessions
            (id, user_id, refresh_token_hash, token_selector, access_token_jti,
             device_fingerprint, expires_at)
        VALUES
            (UUID_TO_BIN(?, true), UUID_TO_BIN(?, true), ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&session_id)
    .bind(user_id.to_string())
    .bind(&refresh_hash)
    .bind(&selector)
    .bind(&jti)
    .bind(device_fingerprint)
    .bind(expires_at)
    .execute(db)
    .await?;

    Ok(TokenPair {
        access_token,
        refresh_token: refresh_raw,
        expires_in: ACCESS_TTL_SECS as u64,
    })
}

/// Rotate a refresh token: validate → revoke old session → issue new pair.
///
/// On reuse of an already-revoked token: revoke ALL user sessions (theft signal).
pub async fn rotate_refresh_token(
    refresh_token: &str,
    device_fingerprint: &str,
    key: &EncodingKey,
    db: &MySqlPool,
) -> Result<TokenPair, AuthError> {
    let selector = sha256_hex(refresh_token);

    let row = sqlx::query(
        r#"
        SELECT
            BIN_TO_UUID(s.id, true)      AS session_id,
            BIN_TO_UUID(s.user_id, true) AS user_id_str,
            s.refresh_token_hash,
            s.revoked_at,
            s.expires_at
        FROM sessions s
        WHERE s.token_selector = ? AND s.expires_at > NOW()
        LIMIT 1
        "#,
    )
    .bind(&selector)
    .fetch_optional(db)
    .await?
    .ok_or(AuthError::SessionNotFound)?;

    let session_id: String = row.try_get("session_id")?;
    let user_id_str: String = row.try_get("user_id_str")?;
    let refresh_hash: String = row.try_get("refresh_token_hash")?;
    let revoked_at: Option<chrono::DateTime<Utc>> = row.try_get("revoked_at")?;

    // Constant-time bcrypt verification.
    let ok = bcrypt::verify(refresh_token, &refresh_hash)
        .map_err(|e| AuthError::Crypto(e.to_string()))?;
    if !ok {
        return Err(AuthError::SessionNotFound);
    }

    if revoked_at.is_some() {
        // Token-theft signal: revoke entire session family.
        sqlx::query(
            "UPDATE sessions SET revoked_at = NOW() \
             WHERE user_id = UUID_TO_BIN(?, true) AND revoked_at IS NULL",
        )
        .bind(&user_id_str)
        .execute(db)
        .await?;
        return Err(AuthError::RefreshTokenReuse);
    }

    // Atomically revoke old session.
    sqlx::query(
        "UPDATE sessions SET revoked_at = NOW() WHERE id = UUID_TO_BIN(?, true)",
    )
    .bind(&session_id)
    .execute(db)
    .await?;

    let user_id = uuid::Uuid::parse_str(&user_id_str)
        .map(KovaUserId::from)
        .map_err(|_| AuthError::SessionNotFound)?;

    issue_token_pair(user_id, device_fingerprint, key, db).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{Algorithm, DecodingKey, Validation};

    fn test_keys() -> (EncodingKey, DecodingKey) {
        let priv_pem = include_bytes!("../tests/test_private_key.pem");
        let pub_pem  = include_bytes!("../tests/test_public_key.pem");
        (
            EncodingKey::from_rsa_pem(priv_pem).unwrap(),
            DecodingKey::from_rsa_pem(pub_pem).unwrap(),
        )
    }

    #[test]
    fn access_token_has_correct_claims() {
        let (enc, dec) = test_keys();
        let user_id = KovaUserId::new();
        let (token, jti) = sign_access_token(user_id, "fp-abc", &enc).unwrap();

        let mut val = Validation::new(Algorithm::RS256);
        val.set_issuer(&["kova-auth"]);
        let data = jsonwebtoken::decode::<KovaClaims>(&token, &dec, &val).unwrap();

        assert_eq!(data.claims.sub, user_id.to_string());
        assert_eq!(data.claims.jti, jti);
        assert_eq!(data.claims.iss, "kova-auth");
        assert_eq!(data.claims.kova_device_id.as_deref(), Some("fp-abc"));

        let ttl = data.claims.exp as i64 - data.claims.iat as i64;
        assert_eq!(ttl, ACCESS_TTL_SECS);
    }

    #[test]
    fn sha256_selector_is_deterministic() {
        let tok = "abc123";
        assert_eq!(sha256_hex(tok), sha256_hex(tok));
        assert_ne!(sha256_hex(tok), sha256_hex("abc124"));
    }
}
