//! POST /api/v1/kova/auth/otp/request  — generate + send OTP
//! POST /api/v1/kova/auth/otp/verify   — verify OTP + issue token pair

use axum::{extract::State, http::StatusCode, Json};
use chrono::{Duration, Utc};
use redis::AsyncCommands;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    crypto::{generate_otp, hash_otp, verify_otp},
    error::AuthError,
    state::AppState,
    tokens::issue_token_pair,
};

/// Rate-limit key format: `kova:otp:rl:<phone_number>`
/// Counter is incremented on each request; TTL is 600s (10 min).
const OTP_RATE_LIMIT: i64 = 3;
const OTP_RATE_WINDOW_SECS: u64 = 600;
const OTP_EXPIRY_SECS: i64 = 300; // 5 minutes

#[derive(Debug, Deserialize)]
pub struct OtpRequestBody {
    pub phone_number: String,
    pub device_fingerprint: String,
}

#[derive(Debug, Deserialize)]
pub struct OtpVerifyBody {
    pub phone_number: String,
    pub otp_code: String,
    pub device_fingerprint: String,
}

/// POST /api/v1/kova/auth/otp/request
///
/// Rate-limited to 3 requests per 10 minutes per phone number.
#[tracing::instrument(skip(state), fields(phone = %body.phone_number))]
pub async fn otp_request(
    State(state): State<AppState>,
    Json(body): Json<OtpRequestBody>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let mut redis = state.redis.clone();

    // Rate limit check.
    let rl_key = format!("kova:otp:rl:{}", body.phone_number);
    let count: Option<i64> = redis.get(&rl_key).await.unwrap_or(None);
    if count.unwrap_or(0) >= OTP_RATE_LIMIT {
        return Err(AuthError::OtpRateLimitExceeded);
    }

    // Look up the user.
    let row = sqlx::query(
        "SELECT BIN_TO_UUID(id, true) AS user_id FROM users WHERE phone_number = ? LIMIT 1",
    )
    .bind(&body.phone_number)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AuthError::UserNotFound)?;

    let user_id_str: String = row.try_get("user_id")?;

    // Generate and hash OTP.
    let otp = generate_otp()?;
    let code_hash = hash_otp(&otp)?;
    let otp_id = Uuid::now_v7().to_string();
    let expires_at = Utc::now() + Duration::seconds(OTP_EXPIRY_SECS);

    sqlx::query(
        r#"
        INSERT INTO otp_codes (id, user_id, code_hash, expires_at)
        VALUES (UUID_TO_BIN(?, true), UUID_TO_BIN(?, true), ?, ?)
        "#,
    )
    .bind(&otp_id)
    .bind(&user_id_str)
    .bind(&code_hash)
    .bind(expires_at)
    .execute(&state.db)
    .await?;

    // Send via SMS provider (mock in dev, real provider in prod).
    state.sms.send_otp(&body.phone_number, &otp).await?;

    // Increment rate-limit counter. Set TTL only on first request in the window.
    let new_count: i64 = redis.incr(&rl_key, 1).await?;
    if new_count == 1 {
        redis
            .expire(&rl_key, OTP_RATE_WINDOW_SECS as i64)
            .await
            .unwrap_or(());
    }

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": { "message": "OTP sent" },
            "error": null,
            "meta": {}
        })),
    ))
}

/// POST /api/v1/kova/auth/otp/verify
///
/// Validates the OTP, marks it used, issues a JWT access + refresh token pair.
#[tracing::instrument(skip(state, body), fields(phone = %body.phone_number))]
pub async fn otp_verify(
    State(state): State<AppState>,
    Json(body): Json<OtpVerifyBody>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    // Look up the user.
    let user_row = sqlx::query(
        "SELECT BIN_TO_UUID(id, true) AS user_id FROM users WHERE phone_number = ? LIMIT 1",
    )
    .bind(&body.phone_number)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AuthError::UserNotFound)?;

    let user_id_str: String = user_row.try_get("user_id")?;

    // Find the most recent unused, unexpired OTP for this user.
    let otp_row = sqlx::query(
        r#"
        SELECT BIN_TO_UUID(id, true) AS otp_id, code_hash, used_at
        FROM otp_codes
        WHERE user_id = UUID_TO_BIN(?, true)
          AND expires_at > NOW()
          AND used_at IS NULL
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(&user_id_str)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AuthError::OtpInvalidOrExpired)?;

    let otp_id: String = otp_row.try_get("otp_id")?;
    let code_hash: String = otp_row.try_get("code_hash")?;
    let used_at: Option<chrono::DateTime<Utc>> = otp_row.try_get("used_at")?;

    if used_at.is_some() {
        return Err(AuthError::OtpAlreadyUsed);
    }

    if !verify_otp(&body.otp_code, &code_hash)? {
        return Err(AuthError::OtpInvalidOrExpired);
    }

    // Mark OTP as used.
    sqlx::query(
        "UPDATE otp_codes SET used_at = NOW() WHERE id = UUID_TO_BIN(?, true)",
    )
    .bind(&otp_id)
    .execute(&state.db)
    .await?;

    let user_id = uuid::Uuid::parse_str(&user_id_str)
        .map(kova_types::KovaUserId::from)
        .map_err(|_| AuthError::UserNotFound)?;

    let pair = issue_token_pair(user_id, &body.device_fingerprint, &state.encoding_key, &state.db)
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": {
                "access_token":  pair.access_token,
                "refresh_token": pair.refresh_token,
                "expires_in":    pair.expires_in,
            },
            "error": null,
            "meta": {}
        })),
    ))
}
