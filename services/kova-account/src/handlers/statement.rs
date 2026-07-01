//! GET /api/v1/kova/accounts/:id/statement
//!
//! Cursor-based pagination (keyset). Cursor is opaque to the client —
//! base64(json({ "created_at": "...", "entry_id": "..." })).
//! Results ordered by created_at DESC (most recent first).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};

use kova_types::ids::KovaAccountId;

use crate::{error::AccountError, handlers::AuthUser, state::AppState};

const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 100;

#[derive(Debug, Deserialize)]
pub struct StatementQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[tracing::instrument(skip(state), fields(user = %user.sub, account_id = %account_id))]
pub async fn get_statement(
    State(state): State<AppState>,
    user: AuthUser,
    Path(account_id): Path<String>,
    Query(q): Query<StatementQuery>,
) -> Result<(StatusCode, Json<Value>), AccountError> {
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(AccountError::InvalidPaginationLimit);
    }

    // Verify ownership.
    sqlx::query(
        "SELECT 1 FROM accounts WHERE id = UUID_TO_BIN(?, true) AND user_id = UUID_TO_BIN(?, true) LIMIT 1",
    )
    .bind(&account_id)
    .bind(&user.sub)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AccountError::AccountNotFound)?;

    let kova_id = uuid::Uuid::parse_str(&account_id)
        .map(KovaAccountId::from)
        .map_err(|_| AccountError::AccountNotFound)?;

    let page = state.ledger.get_statement(kova_id, q.cursor, limit).await?;

    let entries: Vec<Value> = page
        .entries
        .iter()
        .map(|e| {
            json!({
                "entry_id":   e.entry_id,
                "amount":     e.amount.to_string(),
                "direction":  e.direction,
                "description": e.description,
                "created_at": e.created_at.to_rfc3339(),
            })
        })
        .collect();

    // Encode next cursor as base64(json({created_at, entry_id})) if provided by ledger.
    let encoded_cursor = page.next_cursor.as_ref().map(|c| {
        B64.encode(c.as_bytes())
    });

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": entries,
            "error": null,
            "meta": {
                "cursor":   encoded_cursor,
                "has_more": page.has_more,
                "limit":    limit,
            }
        })),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_encoding_is_base64() {
        let raw = r#"{"created_at":"2026-07-01T00:00:00Z","entry_id":"abc"}"#;
        let encoded = B64.encode(raw.as_bytes());
        assert!(!encoded.contains('{'));
        let decoded = String::from_utf8(B64.decode(&encoded).unwrap()).unwrap();
        assert_eq!(decoded, raw);
    }

    #[test]
    fn default_limit_is_20() {
        assert_eq!(DEFAULT_LIMIT, 20);
    }

    #[test]
    fn max_limit_is_100() {
        assert_eq!(MAX_LIMIT, 100);
    }
}
