//! Device management and session revocation.
//!
//! DELETE /api/v1/kova/auth/sessions/:id  — revoke a specific session
//! DELETE /api/v1/kova/auth/sessions      — revoke all sessions (security logout)
//! POST   /api/v1/kova/auth/logout        — revoke current session by JTI

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use redis::AsyncCommands;
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    error::AuthError,
    handlers::AuthClaims,
    state::AppState,
    tokens::ACCESS_TTL_SECS,
};

/// Revocation TTL: access tokens live for 15 min, so 900s is enough to cover
/// any valid token that might have been issued before the session was revoked.
const REVOCATION_CACHE_SECS: u64 = ACCESS_TTL_SECS as u64;

async fn cache_revoked_jti(redis: &mut redis::aio::ConnectionManager, jti: &str) {
    let key = format!("kova:revoked:jti:{jti}");
    redis
        .set_ex::<_, _, ()>(&key, 1u8, REVOCATION_CACHE_SECS)
        .await
        .unwrap_or(());
}

/// DELETE /api/v1/kova/auth/sessions/:id
///
/// Revokes a specific session by its ID. Caches the revocation in Redis so
/// existing access tokens from that session stop working within 60 s.
#[tracing::instrument(skip(state, claims))]
pub async fn session_delete(
    State(state): State<AppState>,
    claims: AuthClaims,
    Path(session_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let mut redis = state.redis.clone();

    // Fetch the session, verifying it belongs to the authenticated user.
    let row = sqlx::query(
        r#"
        SELECT
            BIN_TO_UUID(id, true) AS session_id,
            access_token_jti
        FROM sessions
        WHERE id = UUID_TO_BIN(?, true)
          AND user_id = UUID_TO_BIN(?, true)
          AND revoked_at IS NULL
        LIMIT 1
        "#,
    )
    .bind(&session_id)
    .bind(&claims.0.sub)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AuthError::SessionNotFound)?;

    let jti: Option<String> = row.try_get("access_token_jti")?;

    sqlx::query(
        "UPDATE sessions SET revoked_at = NOW() WHERE id = UUID_TO_BIN(?, true)",
    )
    .bind(&session_id)
    .execute(&state.db)
    .await?;

    if let Some(jti) = jti {
        cache_revoked_jti(&mut redis, &jti).await;
    }

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": { "message": "Session revoked" },
            "error": null,
            "meta": {}
        })),
    ))
}

/// DELETE /api/v1/kova/auth/sessions (no path param)
///
/// Revokes all active sessions for the authenticated user.
#[tracing::instrument(skip(state, claims))]
pub async fn sessions_revoke_all(
    State(state): State<AppState>,
    claims: AuthClaims,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let mut redis = state.redis.clone();

    // Collect JTIs before revoking so we can cache them.
    let rows = sqlx::query(
        r#"
        SELECT access_token_jti
        FROM sessions
        WHERE user_id = UUID_TO_BIN(?, true) AND revoked_at IS NULL
        "#,
    )
    .bind(&claims.0.sub)
    .fetch_all(&state.db)
    .await?;

    sqlx::query(
        "UPDATE sessions SET revoked_at = NOW() \
         WHERE user_id = UUID_TO_BIN(?, true) AND revoked_at IS NULL",
    )
    .bind(&claims.0.sub)
    .execute(&state.db)
    .await?;

    for row in rows {
        if let Ok(Some(jti)) = row.try_get::<Option<String>, _>("access_token_jti") {
            cache_revoked_jti(&mut redis, &jti).await;
        }
    }

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": { "message": "All sessions revoked" },
            "error": null,
            "meta": {}
        })),
    ))
}

/// POST /api/v1/kova/auth/logout
///
/// Revokes the current session identified by the access token's JTI.
#[tracing::instrument(skip(state, claims))]
pub async fn logout(
    State(state): State<AppState>,
    claims: AuthClaims,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let mut redis = state.redis.clone();
    let jti = &claims.0.jti;

    sqlx::query(
        "UPDATE sessions SET revoked_at = NOW() \
         WHERE access_token_jti = ? AND user_id = UUID_TO_BIN(?, true) AND revoked_at IS NULL",
    )
    .bind(jti)
    .bind(&claims.0.sub)
    .execute(&state.db)
    .await?;

    cache_revoked_jti(&mut redis, jti).await;

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": { "message": "Logged out" },
            "error": null,
            "meta": {}
        })),
    ))
}
