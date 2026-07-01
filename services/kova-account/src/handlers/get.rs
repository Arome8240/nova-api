//! GET /api/v1/kova/accounts       — list all accounts for the authenticated user
//! GET /api/v1/kova/accounts/:id   — get a single account

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use sqlx::Row;

use crate::{error::AccountError, handlers::AuthUser, state::AppState};

/// GET /api/v1/kova/accounts
#[tracing::instrument(skip(state), fields(user = %user.sub))]
pub async fn list_accounts(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<(StatusCode, Json<Value>), AccountError> {
    let rows = sqlx::query(
        r#"
        SELECT
            BIN_TO_UUID(id, true)      AS account_id,
            BIN_TO_UUID(user_id, true) AS user_id,
            account_number,
            currency,
            status,
            created_at
        FROM accounts
        WHERE user_id = UUID_TO_BIN(?, true)
        ORDER BY created_at ASC
        "#,
    )
    .bind(&user.sub)
    .fetch_all(&state.db)
    .await?;

    let accounts: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "account_id":     r.try_get::<String, _>("account_id").unwrap_or_default(),
                "account_number": r.try_get::<String, _>("account_number").unwrap_or_default(),
                "currency":       r.try_get::<String, _>("currency").unwrap_or_default(),
                "status":         r.try_get::<String, _>("status").unwrap_or_default(),
                "user_id":        r.try_get::<String, _>("user_id").unwrap_or_default(),
            })
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": accounts,
            "error": null,
            "meta": { "count": accounts.len() }
        })),
    ))
}

/// GET /api/v1/kova/accounts/:id
#[tracing::instrument(skip(state), fields(user = %user.sub, account_id = %account_id))]
pub async fn get_account(
    State(state): State<AppState>,
    user: AuthUser,
    Path(account_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), AccountError> {
    let row = sqlx::query(
        r#"
        SELECT
            BIN_TO_UUID(id, true)      AS account_id,
            BIN_TO_UUID(user_id, true) AS user_id,
            account_number,
            currency,
            status,
            created_at,
            closed_at
        FROM accounts
        WHERE id = UUID_TO_BIN(?, true)
          AND user_id = UUID_TO_BIN(?, true)
        LIMIT 1
        "#,
    )
    .bind(&account_id)
    .bind(&user.sub)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AccountError::AccountNotFound)?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": {
                "account_id":     row.try_get::<String, _>("account_id").unwrap_or_default(),
                "account_number": row.try_get::<String, _>("account_number").unwrap_or_default(),
                "currency":       row.try_get::<String, _>("currency").unwrap_or_default(),
                "status":         row.try_get::<String, _>("status").unwrap_or_default(),
                "user_id":        row.try_get::<String, _>("user_id").unwrap_or_default(),
                "closed_at":      row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("closed_at")
                                    .ok()
                                    .flatten()
                                    .map(|dt| dt.to_rfc3339()),
            },
            "error": null,
            "meta": {}
        })),
    ))
}
