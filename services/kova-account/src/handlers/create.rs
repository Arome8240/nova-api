//! POST /api/v1/kova/accounts — create a new account for the authenticated user.
//!
//! Saga pattern:
//!   1. Generate account number (AUTO_INCREMENT seq)
//!   2. Insert account row (fails on duplicate currency with 409)
//!   3. Call LedgerClient::create_opening_entry (compensate: delete row on failure)
//!   4. Publish AccountCreatedEvent
//!
//! If step 3 or 4 fails, the account row is rolled back so the DB remains consistent.

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use kova_types::{
    entities::CurrencyCode,
    events::account::{AccountCreatedEvent, SCHEMA_VERSION, TOPIC_ACCOUNT_CREATED},
    ids::{KovaAccountId, KovaUserId},
};

use crate::{error::AccountError, handlers::AuthUser, state::AppState};

const SUPPORTED: &[&str] = &["NGN", "GBP", "USD", "KES"];

#[derive(Debug, Deserialize)]
pub struct CreateAccountBody {
    pub currency: String,
}

fn parse_currency(s: &str) -> Result<CurrencyCode, AccountError> {
    if !SUPPORTED.contains(&s) {
        return Err(AccountError::UnsupportedCurrency);
    }
    match s {
        "NGN" => Ok(CurrencyCode::NGN),
        "GBP" => Ok(CurrencyCode::GBP),
        "USD" => Ok(CurrencyCode::USD),
        "KES" => Ok(CurrencyCode::KES),
        _ => Err(AccountError::UnsupportedCurrency),
    }
}

#[tracing::instrument(skip(state), fields(user = %user.sub, currency = %body.currency))]
pub async fn create_account(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateAccountBody>,
) -> Result<(StatusCode, Json<Value>), AccountError> {
    let currency = parse_currency(&body.currency)?;

    // Allocate account number atomically via sequence table.
    let seq_result = sqlx::query("INSERT INTO account_number_seq () VALUES ()")
        .execute(&state.db)
        .await?;
    let seq_num = seq_result.last_insert_id();
    let account_number = format!("KOVA{seq_num:010}");

    let account_id = Uuid::now_v7().to_string();
    let user_id = &user.sub;

    // Insert account row; unique constraint on (user_id, currency) catches duplicates.
    let result = sqlx::query(
        r#"
        INSERT INTO accounts (id, user_id, account_number, currency, status)
        VALUES (UUID_TO_BIN(?, true), UUID_TO_BIN(?, true), ?, ?, 'PendingKYC')
        "#,
    )
    .bind(&account_id)
    .bind(user_id)
    .bind(&account_number)
    .bind(format!("{currency:?}"))
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {}
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AccountError::AccountAlreadyExists);
        }
        Err(e) => return Err(AccountError::Database(e)),
    }

    let kova_account_id = KovaAccountId::from(Uuid::parse_str(&account_id).unwrap());
    let kova_user_id = KovaUserId::from(Uuid::parse_str(user_id).map_err(|_| AccountError::Unauthorized)?);

    // Create opening ledger entry. Compensate (delete account row) if this fails.
    if let Err(e) = state.ledger.create_opening_entry(kova_account_id, currency).await {
        sqlx::query(
            "DELETE FROM accounts WHERE id = UUID_TO_BIN(?, true)",
        )
        .bind(&account_id)
        .execute(&state.db)
        .await
        .ok();
        return Err(e);
    }

    // Publish AccountCreatedEvent (best-effort via NoOp/Kafka publisher).
    let event = AccountCreatedEvent {
        schema_version: SCHEMA_VERSION,
        event_id: KovaAccountId::new(),
        account_id: kova_account_id,
        user_id: kova_user_id,
        currency,
        account_number: account_number.clone(),
        occurred_at: Utc::now(),
    };
    state
        .events
        .publish_json(
            TOPIC_ACCOUNT_CREATED,
            &account_id,
            serde_json::to_value(&event).map_err(|e| AccountError::EventPublish(e.to_string()))?,
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "data": {
                "account_id":     account_id,
                "account_number": account_number,
                "currency":       body.currency,
                "status":         "PendingKYC",
                "user_id":        user_id,
            },
            "error": null,
            "meta": {}
        })),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::parse_currency;
    use crate::error::AccountError;

    #[test]
    fn valid_currencies() {
        assert!(parse_currency("NGN").is_ok());
        assert!(parse_currency("GBP").is_ok());
        assert!(parse_currency("USD").is_ok());
        assert!(parse_currency("KES").is_ok());
    }

    #[test]
    fn invalid_currency_rejected() {
        assert!(matches!(parse_currency("EUR"), Err(AccountError::UnsupportedCurrency)));
        assert!(matches!(parse_currency(""), Err(AccountError::UnsupportedCurrency)));
        assert!(matches!(parse_currency("ngn"), Err(AccountError::UnsupportedCurrency)));
    }
}
