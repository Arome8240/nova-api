use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("phone number already registered")]
    PhoneAlreadyRegistered,

    #[error("invalid phone number format")]
    InvalidPhoneFormat,

    #[error("user not found")]
    UserNotFound,

    #[error("otp invalid or expired")]
    OtpInvalidOrExpired,

    #[error("otp already used")]
    OtpAlreadyUsed,

    #[error("otp rate limit exceeded")]
    OtpRateLimitExceeded,

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("session not found or expired")]
    SessionNotFound,

    #[error("refresh token reuse detected")]
    RefreshTokenReuse,

    #[error("biometric challenge invalid or expired")]
    ChallengeInvalid,

    #[error("pin already set")]
    PinAlreadySet,

    #[error("pin must be 4–6 digits")]
    InvalidPin,

    #[error("pin not set — use POST /auth/pin first")]
    PinNotSet,

    #[error("unauthorized")]
    Unauthorized,

    #[error("database error")]
    Database(#[from] sqlx::Error),

    #[error("redis error")]
    Redis(#[from] redis::RedisError),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("sms delivery error: {0}")]
    Sms(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AuthError::PhoneAlreadyRegistered => (
                StatusCode::CONFLICT,
                "PHONE_ALREADY_REGISTERED",
                "Phone number already registered",
            ),
            AuthError::InvalidPhoneFormat => (
                StatusCode::BAD_REQUEST,
                "INVALID_PHONE_FORMAT",
                "Phone number must be in E.164 format (e.g. +2348012345678)",
            ),
            AuthError::UserNotFound => (
                StatusCode::NOT_FOUND,
                "USER_NOT_FOUND",
                "No account found for this phone number",
            ),
            AuthError::OtpInvalidOrExpired => (
                StatusCode::UNAUTHORIZED,
                "OTP_INVALID",
                "OTP is invalid or expired",
            ),
            AuthError::OtpAlreadyUsed => {
                (StatusCode::CONFLICT, "OTP_ALREADY_USED", "OTP has already been used")
            }
            AuthError::OtpRateLimitExceeded => (
                StatusCode::TOO_MANY_REQUESTS,
                "OTP_RATE_LIMIT",
                "Too many OTP requests — please wait before requesting again",
            ),
            AuthError::InvalidCredentials => {
                (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS", "Invalid credentials")
            }
            AuthError::SessionNotFound => {
                (StatusCode::UNAUTHORIZED, "SESSION_NOT_FOUND", "Session not found or expired")
            }
            AuthError::RefreshTokenReuse => (
                StatusCode::UNAUTHORIZED,
                "REFRESH_TOKEN_REUSE",
                "Refresh token reuse detected — all sessions have been revoked",
            ),
            AuthError::ChallengeInvalid => (
                StatusCode::UNAUTHORIZED,
                "CHALLENGE_INVALID",
                "Biometric challenge is invalid or expired",
            ),
            AuthError::PinAlreadySet => {
                (StatusCode::CONFLICT, "PIN_ALREADY_SET", "PIN has already been set for this account")
            }
            AuthError::InvalidPin => (
                StatusCode::BAD_REQUEST,
                "INVALID_PIN",
                "PIN must be 4–6 numeric digits",
            ),
            AuthError::PinNotSet => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "PIN_NOT_SET",
                "No PIN is set — use POST /api/v1/kova/auth/pin to set one",
            ),
            AuthError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Missing or invalid access token")
            }
            AuthError::Database(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", "A database error occurred")
            }
            AuthError::Redis(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "CACHE_ERROR",
                "A cache error occurred",
            ),
            AuthError::Crypto(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "CRYPTO_ERROR",
                "A cryptographic error occurred",
            ),
            AuthError::Sms(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "SMS_ERROR",
                "Failed to deliver OTP — please try again",
            ),
        };

        let body = json!({
            "data": null,
            "error": { "code": code, "message": message },
            "meta": {}
        });

        (status, Json(body)).into_response()
    }
}
