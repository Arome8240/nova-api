//! Handler stubs — all return 501 Not Implemented until the upstream services
//! are wired up in Deliverables 6–14.

use axum::response::Response;
use crate::error::ApiError;

pub async fn not_implemented() -> Response {
    ApiError::not_implemented()
}
