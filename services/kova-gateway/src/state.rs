//! Shared application state threaded through the Axum router.

use redis::aio::ConnectionManager;

use crate::middleware::auth::JwtKeySet;

#[derive(Clone)]
pub struct AppState {
    pub jwt_keys: JwtKeySet,
    pub redis: ConnectionManager,
}
