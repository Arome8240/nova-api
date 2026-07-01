use axum::{
    routing::{delete, get, post, put},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::{
    handlers::{
        biometric::{biometric_challenge, biometric_verify},
        device::{logout, session_delete, sessions_revoke_all},
        health::health_check,
        otp::{otp_request, otp_verify},
        pin::{pin_change, pin_set},
        register::register,
        token::token_refresh,
    },
    state::AppState,
};

pub fn build(state: AppState) -> Router {
    Router::new()
        // Health (unauthenticated)
        .route("/api/v1/kova/auth/health", get(health_check))
        // Registration (unauthenticated)
        .route("/api/v1/kova/auth/register", post(register))
        // OTP (unauthenticated)
        .route("/api/v1/kova/auth/otp/request", post(otp_request))
        .route("/api/v1/kova/auth/otp/verify", post(otp_verify))
        // Token (unauthenticated — uses refresh token for auth)
        .route("/api/v1/kova/auth/token/refresh", post(token_refresh))
        // Biometric (requires valid access token)
        .route("/api/v1/kova/auth/biometric/challenge", post(biometric_challenge))
        .route("/api/v1/kova/auth/biometric/verify", post(biometric_verify))
        // Session management (requires valid access token)
        .route("/api/v1/kova/auth/sessions/{id}", delete(session_delete))
        .route("/api/v1/kova/auth/sessions", delete(sessions_revoke_all))
        .route("/api/v1/kova/auth/logout", post(logout))
        // PIN management (requires valid access token)
        .route("/api/v1/kova/auth/pin", post(pin_set))
        .route("/api/v1/kova/auth/pin", put(pin_change))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
