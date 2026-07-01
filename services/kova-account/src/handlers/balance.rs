//! GET /api/v1/kova/accounts/:id/balance
//!
//! Balance is ALWAYS sourced from kova-ledger, never from kova_account_db.
//! Cached in Redis for 5 seconds. On ledger failure, returns stale cached value
//! with `meta.stale: true`.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use redis::AsyncCommands;
use serde_json::{json, Value};
use sqlx::Row;

use kova_types::ids::KovaAccountId;

use crate::{error::AccountError, handlers::AuthUser, state::AppState};

const BALANCE_CACHE_TTL_SECS: u64 = 5;

fn balance_cache_key(account_id: &str) -> String {
    format!("kova:balance:{account_id}")
}

#[tracing::instrument(skip(state), fields(user = %user.sub, account_id = %account_id))]
pub async fn get_balance(
    State(state): State<AppState>,
    user: AuthUser,
    Path(account_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), AccountError> {
    // Verify ownership.
    let row = sqlx::query(
        r#"
        SELECT currency FROM accounts
        WHERE id = UUID_TO_BIN(?, true)
          AND user_id = UUID_TO_BIN(?, true)
          AND status != 'Closed'
        LIMIT 1
        "#,
    )
    .bind(&account_id)
    .bind(&user.sub)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AccountError::AccountNotFound)?;

    let currency: String = row.try_get("currency")?;

    let cache_key = balance_cache_key(&account_id);
    let mut redis = state.redis.clone();

    // Cache hit.
    if let Ok(Some(cached)) = redis.get::<_, Option<String>>(&cache_key).await {
        if let Ok(v) = serde_json::from_str::<Value>(&cached) {
            return Ok((StatusCode::OK, Json(json!({
                "data": v,
                "error": null,
                "meta": {}
            }))));
        }
    }

    let kova_id = uuid::Uuid::parse_str(&account_id)
        .map(KovaAccountId::from)
        .map_err(|_| AccountError::AccountNotFound)?;

    match state.ledger.get_balance(kova_id).await {
        Ok(bal) => {
            let data = json!({
                "account_id":        account_id,
                "currency":          currency,
                "available_balance": bal.available_balance.to_string(),
                "pending_amount":    bal.pending_amount.to_string(),
            });

            // Cache the result.
            redis
                .set_ex::<_, _, ()>(&cache_key, data.to_string(), BALANCE_CACHE_TTL_SECS)
                .await
                .ok();

            Ok((StatusCode::OK, Json(json!({
                "data": data,
                "error": null,
                "meta": {}
            }))))
        }
        Err(e) => {
            // On ledger failure, return stale cache if available.
            if let Ok(Some(cached)) = redis.get::<_, Option<String>>(&cache_key).await {
                if let Ok(v) = serde_json::from_str::<Value>(&cached) {
                    return Ok((StatusCode::OK, Json(json!({
                        "data": v,
                        "error": null,
                        "meta": { "stale": true }
                    }))));
                }
            }
            Err(e)
        }
    }
}
