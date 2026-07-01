//! POST /api/v1/kova/auth/token/refresh — rotate a refresh token

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{error::AuthError, state::AppState, tokens::rotate_refresh_token};

#[derive(Debug, Deserialize)]
pub struct TokenRefreshBody {
    pub refresh_token: String,
    pub device_fingerprint: String,
}

/// POST /api/v1/kova/auth/token/refresh
///
/// Validates and rotates the refresh token. Reuse of a previously-rotated
/// token revokes the entire session family.
#[tracing::instrument(skip(state, body))]
pub async fn token_refresh(
    State(state): State<AppState>,
    Json(body): Json<TokenRefreshBody>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let pair = rotate_refresh_token(
        &body.refresh_token,
        &body.device_fingerprint,
        &state.encoding_key,
        &state.db,
    )
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
