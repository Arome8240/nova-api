//! PIN management endpoints.
//!
//! POST /api/v1/kova/auth/pin — set initial PIN (requires OTP re-verification)
//! PUT  /api/v1/kova/auth/pin — change PIN (requires current PIN + OTP re-verification)

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    crypto::{hash_pin, verify_otp, verify_pin},
    error::AuthError,
    handlers::AuthClaims,
    state::AppState,
};

fn validate_pin_format(pin: &str) -> Result<(), AuthError> {
    let len = pin.len();
    if !(4..=6).contains(&len) || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(AuthError::InvalidPin);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct SetPinBody {
    #[serde(rename = "pin")]
    pub new_pin: String,
    /// OTP required to confirm the PIN set operation.
    pub otp_code: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePinBody {
    pub current_pin: String,
    pub new_pin: String,
    /// OTP required before the PIN change is committed.
    pub otp_code: String,
}

/// POST /api/v1/kova/auth/pin
///
/// Sets the initial PIN for a newly registered user.
/// Requires valid access token + OTP re-verification.
#[tracing::instrument(skip(state, body), fields(sub = %claims.0.sub))]
pub async fn pin_set(
    State(state): State<AppState>,
    claims: AuthClaims,
    Json(body): Json<SetPinBody>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    validate_pin_format(&body.new_pin)?;

    let row = sqlx::query(
        "SELECT pin_hash FROM users WHERE id = UUID_TO_BIN(?, true) LIMIT 1",
    )
    .bind(&claims.0.sub)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AuthError::UserNotFound)?;

    let pin_hash: String = row.try_get("pin_hash")?;
    if !pin_hash.is_empty() {
        return Err(AuthError::PinAlreadySet);
    }

    // OTP re-verification before committing the PIN set.
    verify_otp_for_user(&claims.0.sub, &body.otp_code, &state).await?;

    let new_hash = hash_pin(&body.new_pin)?;
    sqlx::query(
        "UPDATE users SET pin_hash = ? WHERE id = UUID_TO_BIN(?, true)",
    )
    .bind(&new_hash)
    .bind(&claims.0.sub)
    .execute(&state.db)
    .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": { "message": "PIN set successfully" },
            "error": null,
            "meta": {}
        })),
    ))
}

/// PUT /api/v1/kova/auth/pin
///
/// Changes an existing PIN.
/// Requires valid access token + current PIN verification + OTP re-verification.
#[tracing::instrument(skip(state, body), fields(sub = %claims.0.sub))]
pub async fn pin_change(
    State(state): State<AppState>,
    claims: AuthClaims,
    Json(body): Json<ChangePinBody>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    validate_pin_format(&body.new_pin)?;

    let row = sqlx::query(
        "SELECT pin_hash FROM users WHERE id = UUID_TO_BIN(?, true) LIMIT 1",
    )
    .bind(&claims.0.sub)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AuthError::UserNotFound)?;

    let pin_hash: String = row.try_get("pin_hash")?;
    if pin_hash.is_empty() {
        return Err(AuthError::PinNotSet);
    }

    if !verify_pin(&body.current_pin, &pin_hash)? {
        return Err(AuthError::InvalidCredentials);
    }

    // OTP re-verification before committing PIN change.
    verify_otp_for_user(&claims.0.sub, &body.otp_code, &state).await?;

    let new_hash = hash_pin(&body.new_pin)?;
    sqlx::query(
        "UPDATE users SET pin_hash = ? WHERE id = UUID_TO_BIN(?, true)",
    )
    .bind(&new_hash)
    .bind(&claims.0.sub)
    .execute(&state.db)
    .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": { "message": "PIN changed successfully" },
            "error": null,
            "meta": {}
        })),
    ))
}

/// Verify the most recent unused, unexpired OTP for a given user_id string
/// and mark it used. Called by PIN set/change to enforce OTP re-verification.
async fn verify_otp_for_user(
    user_id: &str,
    otp_code: &str,
    state: &AppState,
) -> Result<(), AuthError> {
    let row = sqlx::query(
        r#"
        SELECT BIN_TO_UUID(id, true) AS otp_id, code_hash
        FROM otp_codes
        WHERE user_id = UUID_TO_BIN(?, true)
          AND expires_at > NOW()
          AND used_at IS NULL
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AuthError::OtpInvalidOrExpired)?;

    let otp_id: String = row.try_get("otp_id")?;
    let code_hash: String = row.try_get("code_hash")?;

    if !verify_otp(otp_code, &code_hash)? {
        return Err(AuthError::OtpInvalidOrExpired);
    }

    sqlx::query(
        "UPDATE otp_codes SET used_at = NOW() WHERE id = UUID_TO_BIN(?, true)",
    )
    .bind(&otp_id)
    .execute(&state.db)
    .await?;

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::validate_pin_format;
    use crate::error::AuthError;

    #[test]
    fn valid_pins() {
        assert!(validate_pin_format("1234").is_ok());
        assert!(validate_pin_format("12345").is_ok());
        assert!(validate_pin_format("123456").is_ok());
    }

    #[test]
    fn pin_too_short() {
        assert!(matches!(validate_pin_format("123"), Err(AuthError::InvalidPin)));
    }

    #[test]
    fn pin_too_long() {
        assert!(matches!(validate_pin_format("1234567"), Err(AuthError::InvalidPin)));
    }

    #[test]
    fn pin_with_letters_rejected() {
        assert!(matches!(validate_pin_format("12ab"), Err(AuthError::InvalidPin)));
    }

    #[test]
    fn empty_pin_rejected() {
        assert!(matches!(validate_pin_format(""), Err(AuthError::InvalidPin)));
    }
}
