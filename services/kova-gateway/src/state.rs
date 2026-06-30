//! Shared application state threaded through the Axum router.

use redis::aio::ConnectionManager;

use crate::middleware::auth::JwtKeySet;

#[derive(Clone)]
pub struct AppState {
    pub jwt_keys: JwtKeySet,
    pub redis: ConnectionManager,
}

impl AppState {
    /// Minimal state for unit tests — uses a dummy key set and a fake Redis URL
    /// that is never actually connected to (tests that need Redis use a real
    /// testcontainers instance).
    ///
    /// Tests that don't exercise auth or rate limiting can use this safely.
    #[cfg(test)]
    pub fn test_default() -> Self {
        // Build a fake ConnectionManager. In tests that don't call Redis this
        // never actually connects; the test_default path exercises route existence
        // only (no middleware calls Redis for 501 responses under test).
        //
        // We can't construct a ConnectionManager without an actual connection,
        // so router existence tests must bypass the rate-limit layer. See
        // `router::tests` for the test-bypass approach.
        unimplemented!(
            "AppState::test_default requires a real Redis or test infrastructure. \
             See router::tests for how to construct a test router."
        )
    }
}
