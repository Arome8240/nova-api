//! POST /api/v1/kova/auth/register
//!
//! Phone-first user registration. Validates E.164 format, checks uniqueness,
//! inserts the user row, and registers the device fingerprint.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{error::AuthError, state::AppState};

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub phone_number: String,
    pub device_fingerprint: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub user_id: String,
    pub phone_number: String,
}

/// Validate E.164 phone number and return the canonical form.
fn validate_e164(raw: &str) -> Result<String, AuthError> {
    let phone = phonenumber::parse(None, raw).map_err(|_| AuthError::InvalidPhoneFormat)?;
    if !phonenumber::is_valid(&phone) {
        return Err(AuthError::InvalidPhoneFormat);
    }
    // Return canonical E.164 string (e.g., "+2348012345678")
    Ok(phone
        .format()
        .mode(phonenumber::Mode::E164)
        .to_string())
}

#[tracing::instrument(skip(state), fields(phone = %req.phone_number))]
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let phone = validate_e164(&req.phone_number)?;

    let user_id = Uuid::now_v7().to_string();

    // Insert user — unique constraint on phone_number catches duplicates.
    let result = sqlx::query(
        r#"
        INSERT INTO users (id, phone_number, pin_hash, kyc_status)
        VALUES (UUID_TO_BIN(?, true), ?, '', 'Unverified')
        "#,
    )
    .bind(&user_id)
    .bind(&phone)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {}
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AuthError::PhoneAlreadyRegistered);
        }
        Err(e) => return Err(AuthError::Database(e)),
    }

    // Register the device fingerprint.
    let device_id = Uuid::now_v7().to_string();
    sqlx::query(
        r#"
        INSERT INTO devices (id, user_id, device_fingerprint)
        VALUES (UUID_TO_BIN(?, true), UUID_TO_BIN(?, true), ?)
        ON DUPLICATE KEY UPDATE last_seen_at = NOW()
        "#,
    )
    .bind(&device_id)
    .bind(&user_id)
    .bind(&req.device_fingerprint)
    .execute(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "data": { "user_id": user_id, "phone_number": phone },
            "error": null,
            "meta": {}
        })),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::validate_e164;
    use crate::error::AuthError;

    #[test]
    fn valid_nigerian_number() {
        let e = validate_e164("+2348012345678").unwrap();
        assert_eq!(e, "+2348012345678");
    }

    #[test]
    fn valid_kenyan_number() {
        assert!(validate_e164("+254712345678").is_ok());
    }

    #[test]
    fn valid_ghanaian_number() {
        assert!(validate_e164("+233541234567").is_ok());
    }

    #[test]
    fn valid_south_african_number() {
        assert!(validate_e164("+27821234567").is_ok());
    }

    #[test]
    fn local_format_rejected() {
        assert!(matches!(
            validate_e164("08012345678"),
            Err(AuthError::InvalidPhoneFormat)
        ));
    }

    #[test]
    fn letters_rejected() {
        assert!(matches!(
            validate_e164("+234abc12345"),
            Err(AuthError::InvalidPhoneFormat)
        ));
    }

    #[test]
    fn empty_string_rejected() {
        assert!(matches!(
            validate_e164(""),
            Err(AuthError::InvalidPhoneFormat)
        ));
    }
}
