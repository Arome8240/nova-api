//! DELETE /api/v1/kova/accounts/:id — account closure workflow (TASK-050).
//!
//! Sequence (all checks before any DB write):
//!   1. Verify ownership + current status (not already Closed)
//!   2. Check no pending payments exist (PaymentsClient)
//!   3. Check balance is zero (LedgerClient)
//!   4. Apply Closed transition via state machine
//!   5. Persist status + closed_at to DB
//!   6. Publish AccountClosedEvent
//!
//! Closure is irreversible: Closed → * transitions are rejected by the state machine.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::Row;

use kova_types::{
    entities::AccountStatus,
    events::account::{AccountClosedEvent, SCHEMA_VERSION, TOPIC_ACCOUNT_CLOSED},
    ids::{KovaAccountId, KovaUserId},
};

use crate::{
    error::AccountError,
    handlers::AuthUser,
    state::AppState,
    state_machine::transition,
};

#[tracing::instrument(skip(state), fields(user = %user.sub, account_id = %account_id))]
pub async fn close_account(
    State(state): State<AppState>,
    user: AuthUser,
    Path(account_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), AccountError> {
    // Step 1 — verify ownership and current status.
    let row = sqlx::query(
        r#"
        SELECT
            BIN_TO_UUID(user_id, true) AS user_id,
            status
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

    let current_status_str: String = row.try_get("status")?;
    let current_status = parse_status(&current_status_str)?;

    if current_status == AccountStatus::Closed {
        return Err(AccountError::AlreadyClosed);
    }

    let kova_id = uuid::Uuid::parse_str(&account_id)
        .map(KovaAccountId::from)
        .map_err(|_| AccountError::AccountNotFound)?;
    let kova_user_id = uuid::Uuid::parse_str(&user.sub)
        .map(KovaUserId::from)
        .map_err(|_| AccountError::Unauthorized)?;

    // Step 2 — check for pending payments.
    if state.payments.has_pending_payments(kova_id).await? {
        return Err(AccountError::PendingPaymentsExist);
    }

    // Step 3 — verify zero balance.
    let balance = state.ledger.get_balance(kova_id).await?;
    if !balance.available_balance.is_zero() || !balance.pending_amount.is_zero() {
        return Err(AccountError::AccountHasBalance {
            balance: balance.available_balance.to_string(),
        });
    }

    // Step 4 — validate state machine transition.
    transition(current_status, AccountStatus::Closed).map_err(|e| {
        AccountError::InvalidTransition {
            from: e.from,
            to: e.to,
        }
    })?;

    // Step 5 — persist.
    let closed_at = Utc::now();
    sqlx::query(
        "UPDATE accounts SET status = 'Closed', closed_at = ? WHERE id = UUID_TO_BIN(?, true)",
    )
    .bind(closed_at)
    .bind(&account_id)
    .execute(&state.db)
    .await?;

    // Step 6 — publish event.
    let event = AccountClosedEvent {
        schema_version: SCHEMA_VERSION,
        event_id: KovaAccountId::new(),
        account_id: kova_id,
        user_id: kova_user_id,
        reason: "user_initiated".to_string(),
        closed_at,
        occurred_at: closed_at,
    };
    state
        .events
        .publish_json(
            TOPIC_ACCOUNT_CLOSED,
            &account_id,
            serde_json::to_value(&event).map_err(|e| AccountError::EventPublish(e.to_string()))?,
        )
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "data": {
                "account_id": account_id,
                "status": "Closed",
                "closed_at": closed_at.to_rfc3339(),
            },
            "error": null,
            "meta": {}
        })),
    ))
}

fn parse_status(s: &str) -> Result<AccountStatus, AccountError> {
    match s {
        "Active" => Ok(AccountStatus::Active),
        "Frozen" => Ok(AccountStatus::Frozen),
        "Closed" => Ok(AccountStatus::Closed),
        "PendingKYC" => Ok(AccountStatus::PendingKYC),
        other => Err(AccountError::InvalidTransition {
            from: other.to_string(),
            to: "Closed".to_string(),
        }),
    }
}
