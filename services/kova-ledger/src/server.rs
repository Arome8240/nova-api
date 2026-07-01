//! TASK-057 — gRPC service implementation.
//!
//! `LedgerService` implements the `KovaLedger` tonic trait generated from
//! `proto/kova_ledger.proto`.
//!
//! Internal JWT auth is enforced via a tonic interceptor that validates
//! the `Authorization: Bearer <token>` metadata header on every call.  The
//! `DecodingKey` is cloned into the interceptor closure at startup; RS256 +
//! issuer "kova-auth" are required.
//!
//! The service itself is a thin adapter: it converts proto bytes/strings into
//! Rust types, delegates to the business-logic functions in the sibling
//! modules, and converts results back to proto messages.

use std::str::FromStr;
use chrono::DateTime;
use prost_types::Timestamp;
use rust_decimal::Decimal;
use sqlx::MySqlPool;
use tonic::{Request, Response, Status};

use crate::{
    balance::get_balance,
    posting::{post_entry, PostEntryParams},
    proto::{
        kova_ledger_server::KovaLedger,
        GetBalanceRequest, GetBalanceResponse, GetStatementRequest, GetStatementResponse,
        LedgerEntryProto, PostEntryRequest, PostEntryResponse,
    },
    statement::get_statement,
};

// ── Service struct ────────────────────────────────────────────────────────────

pub struct LedgerService {
    pool: MySqlPool,
}

impl LedgerService {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }
}

// ── gRPC implementation ───────────────────────────────────────────────────────

#[tonic::async_trait]
impl KovaLedger for LedgerService {
    async fn post_entry(
        &self,
        request: Request<PostEntryRequest>,
    ) -> Result<Response<PostEntryResponse>, Status> {
        let req = request.into_inner();

        let debit_account_id = bytes_to_id(&req.debit_account_id)?;
        let credit_account_id = bytes_to_id(&req.credit_account_id)?;
        let amount = Decimal::from_str(&req.amount)
            .map_err(|e| Status::invalid_argument(format!("invalid amount: {e}")))?;

        let payment_id = if req.payment_id.is_empty() {
            None
        } else {
            Some(bytes_to_id(&req.payment_id)?)
        };

        let metadata_json = if req.metadata_json.is_empty() {
            None
        } else {
            Some(req.metadata_json)
        };

        let params = PostEntryParams {
            debit_account_id,
            credit_account_id,
            amount,
            currency: req.currency,
            idempotency_key: req.idempotency_key,
            payment_id,
            metadata_json,
        };

        let result = post_entry(&self.pool, params).await.map_err(Status::from)?;

        Ok(Response::new(PostEntryResponse {
            entry_id: result.entry_id.to_vec(),
            was_idempotent: result.was_idempotent,
            created_at: Some(dt_to_timestamp(result.created_at)),
        }))
    }

    async fn get_balance(
        &self,
        request: Request<GetBalanceRequest>,
    ) -> Result<Response<GetBalanceResponse>, Status> {
        let req = request.into_inner();
        let account_id = bytes_to_id(&req.account_id)?;

        let result = get_balance(&self.pool, &account_id, &req.currency)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(GetBalanceResponse {
            account_id: account_id.to_vec(),
            currency: req.currency,
            balance: result.balance.to_string(),
            entry_count: result.entry_count,
            as_of: result.as_of.map(dt_to_timestamp),
        }))
    }

    async fn get_statement(
        &self,
        request: Request<GetStatementRequest>,
    ) -> Result<Response<GetStatementResponse>, Status> {
        let req = request.into_inner();
        let account_id = bytes_to_id(&req.account_id)?;

        let limit = if req.limit == 0 { 20 } else { req.limit };

        let currency = if req.currency.is_empty() {
            None
        } else {
            Some(req.currency.as_str())
        };

        let cursor_opt: Option<String> = if req.cursor.is_empty() {
            None
        } else {
            Some(req.cursor)
        };

        let from_time = req.from_time.and_then(timestamp_to_dt);
        let to_time = req.to_time.and_then(timestamp_to_dt);

        let page = get_statement(
            &self.pool,
            &account_id,
            currency,
            cursor_opt.as_deref(),
            limit,
            from_time,
            to_time,
        )
        .await
        .map_err(Status::from)?;

        let entries: Vec<LedgerEntryProto> = page
            .entries
            .into_iter()
            .map(|e| LedgerEntryProto {
                entry_id:          e.entry_id.to_vec(),
                debit_account_id:  e.debit_account_id.to_vec(),
                credit_account_id: e.credit_account_id.to_vec(),
                amount:            e.amount.to_string(),
                currency:          e.currency,
                idempotency_key:   e.idempotency_key,
                payment_id:        e.payment_id.map(|id| id.to_vec()).unwrap_or_default(),
                metadata_json:     e.metadata_json.unwrap_or_default(),
                created_at:        Some(dt_to_timestamp(e.created_at)),
            })
            .collect();

        Ok(Response::new(GetStatementResponse {
            entries,
            next_cursor: page.next_cursor.unwrap_or_default(),
            has_more: page.has_more,
        }))
    }
}

// ── Auth interceptor ──────────────────────────────────────────────────────────

/// Create the inner service.  Callers wrap it with the interceptor before
/// passing to `tonic::transport::Server::add_service`.
pub fn make_service(pool: MySqlPool) -> LedgerService {
    LedgerService::new(pool)
}

/// Build an RS256 JWT interceptor that validates every inbound gRPC call.
///
/// Attach via `KovaLedgerServer::with_interceptor(service, interceptor)`.
/// Issuer must be `"kova-auth"`.
pub fn make_auth_interceptor(
    jwt_public_key_pem: &[u8],
) -> Result<
    impl FnMut(Request<()>) -> Result<Request<()>, Status> + Clone,
    anyhow::Error,
> {
    use jsonwebtoken::{Algorithm, DecodingKey, Validation};

    let decoding_key = DecodingKey::from_rsa_pem(jwt_public_key_pem)
        .map_err(|e| anyhow::anyhow!("invalid JWT_PUBLIC_KEY: {e}"))?;

    Ok(move |req: Request<()>| -> Result<Request<()>, Status> {
        let token = req
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or_else(|| Status::unauthenticated("missing Authorization header"))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&["kova-auth"]);

        jsonwebtoken::decode::<serde_json::Value>(token, &decoding_key, &validation)
            .map_err(|_| Status::unauthenticated("invalid or expired token"))?;

        Ok(req)
    })
}

// ── Type conversion helpers ───────────────────────────────────────────────────

fn bytes_to_id(b: &[u8]) -> Result<[u8; 16], Status> {
    b.try_into()
        .map_err(|_| Status::invalid_argument("UUID field must be exactly 16 bytes"))
}

fn dt_to_timestamp(dt: DateTime<chrono::Utc>) -> Timestamp {
    Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}

fn timestamp_to_dt(ts: Timestamp) -> Option<DateTime<chrono::Utc>> {
    DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
}
