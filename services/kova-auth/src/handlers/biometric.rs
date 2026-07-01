//! POST /api/v1/kova/auth/biometric/challenge — issue a 60-second challenge token
//! POST /api/v1/kova/auth/biometric/verify   — verify challenge + issue token pair

use axum::{extract::State, http::StatusCode, Json};
use redis::AsyncCommands;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    crypto::random_hex_token,
    error::AuthError,
    handlers::AuthClaims,
    state::AppState,
    tokens::issue_token_pair,
};

const CHALLENGE_TTL_SECS: u64 = 60;

fn challenge_key(user_id: &str) -> String {
    format!("kova:biometric:challenge:{user_id}")
}

#[derive(Debug, Deserialize)]
pub struct BiometricVerifyBody {
    pub challenge_token: String,
    /// Opaque biometric signature from the device OS — not inspected server-side.
    pub biometric_signature: String,
    pub device_fingerprint: String,
}

/// POST /api/v1/kova/auth/biometric/challenge
///
/// Issues a 60-second single-use challenge token stored in Redis.
/// Requires a valid access token.
#[tracing::instrument(skip(state, claims))]
pub async fn biometric_challenge(
    State(state): State<AppState>,
    claims: AuthClaims,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let mut redis = state.redis.clone();
    let user_id = &claims.0.sub;

    let challenge = random_hex_token()?;
    let key = challenge_key(user_id);

    redis
        .set_ex::<_, _, ()>(&key, &challenge, CHALLENGE_TTL_SECS)
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": { "challenge_token": challenge },
            "error": null,
            "meta": {}
        })),
    ))
}

/// POST /api/v1/kova/auth/biometric/verify
///
/// Validates the challenge exists in Redis (single-use DEL), then issues a
/// fresh token pair. The biometric_signature is treated as opaque — the device
/// OS guarantees the biometric check happened.
///
/// Requires a valid access token (the user must have logged in at least once).
#[tracing::instrument(skip(state, body, claims))]
pub async fn biometric_verify(
    State(state): State<AppState>,
    claims: AuthClaims,
    Json(body): Json<BiometricVerifyBody>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let mut redis = state.redis.clone();
    let user_id = &claims.0.sub;
    let key = challenge_key(user_id);

    // Retrieve and atomically delete the challenge (single-use).
    let stored: Option<String> = redis.get_del(&key).await?;

    match stored {
        None => return Err(AuthError::ChallengeInvalid),
        Some(ref stored_challenge) if stored_challenge != &body.challenge_token => {
            return Err(AuthError::ChallengeInvalid);
        }
        _ => {}
    }

    let user_uuid = uuid::Uuid::parse_str(user_id)
        .map(kova_types::KovaUserId::from)
        .map_err(|_| AuthError::Unauthorized)?;

    let pair =
        issue_token_pair(user_uuid, &body.device_fingerprint, &state.encoding_key, &state.db)
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
