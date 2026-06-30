//! Unified JSON error response type used by all gateway handlers and middleware.
//!
//! Every error response follows the envelope:
//!   `{ "data": null, "error": { "code": "...", "message": "..." }, "meta": {} }`

use axum::{
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub data: serde_json::Value,
    pub error: ErrorBody,
    pub meta: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            status,
            Json(Self {
                data: serde_json::Value::Null,
                error: ErrorBody {
                    code: code.into(),
                    message: message.into(),
                },
                meta: serde_json::json!({}),
            }),
        )
    }

    pub fn not_implemented() -> Response {
        let (status, body) = Self::new(
            StatusCode::NOT_IMPLEMENTED,
            "NOT_IMPLEMENTED",
            "This endpoint is not yet implemented",
        );
        (status, body).into_response()
    }

    pub fn unauthorized(code: &str, message: &str) -> Response {
        let (status, body) = Self::new(StatusCode::UNAUTHORIZED, code, message);
        (status, body).into_response()
    }

    pub fn too_many_requests(retry_after_secs: u64) -> Response {
        use axum::response::AppendHeaders;
        use http::header;
        let (status, body) = Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMIT_EXCEEDED",
            "Too many requests — please slow down",
        );
        (
            status,
            AppendHeaders([(
                header::RETRY_AFTER,
                retry_after_secs.to_string(),
            )]),
            body,
        )
            .into_response()
    }

    pub fn service_unavailable(service: &str) -> Response {
        let (status, body) = (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "data": null,
                "error": {
                    "code": "SERVICE_UNAVAILABLE",
                    "message": format!("upstream service '{service}' is temporarily unavailable")
                },
                "meta": {}
            })),
        );
        (status, body).into_response()
    }
}
