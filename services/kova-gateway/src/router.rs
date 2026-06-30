//! KOVA API Gateway — complete route table (TASK-029).
//!
//! All routes are under `/api/v1/kova/`. Path parameters are UUID-typed at
//! the handler boundary; a non-UUID value returns 400 before reaching handler
//! logic (enforced by `KovaAccountId::try_from` in each handler).
//!
//! Handler stubs return 501 until the upstream services are wired (TASK-037+).

use axum::{
    routing::{delete, get, post, put},
    Router,
};

use crate::handlers::stubs::not_implemented;

/// Build the inner route tree (no middleware). Used directly in tests.
pub fn routes() -> Router {
    Router::new()
        // ── Auth (unauthenticated) ────────────────────────────────────────────
        .route("/auth/register", post(not_implemented))
        .route("/auth/otp/request", post(not_implemented))
        .route("/auth/otp/verify", post(not_implemented))
        .route("/auth/token/refresh", post(not_implemented))
        .route("/auth/sessions/{id}", delete(not_implemented))
        .route("/auth/biometric/challenge", post(not_implemented))
        .route("/auth/biometric/verify", post(not_implemented))
        // ── Accounts ─────────────────────────────────────────────────────────
        .route("/accounts", post(not_implemented))
        .route("/accounts/{id}", get(not_implemented))
        .route("/accounts/{id}/balance", get(not_implemented))
        .route("/accounts/{id}/statement", get(not_implemented))
        // ── Payments ─────────────────────────────────────────────────────────
        .route("/payments", post(not_implemented))
        .route("/payments/{id}", get(not_implemented))
        // ── Cards ─────────────────────────────────────────────────────────────
        .route("/cards", post(not_implemented))
        .route("/cards/{id}", get(not_implemented))
        .route("/cards/{id}/freeze", post(not_implemented))
        .route("/cards/{id}/freeze", delete(not_implemented))
        .route("/cards/{id}/transactions", get(not_implemented))
        .route("/cards/{id}/spend-controls", put(not_implemented))
        // ── KYC ──────────────────────────────────────────────────────────────
        .route("/kyc/submit", post(not_implemented))
        .route("/kyc/status", get(not_implemented))
        // ── FX ───────────────────────────────────────────────────────────────
        .route("/fx/quote", post(not_implemented))
        .route("/fx/quotes/{id}", get(not_implemented))
        // ── Health ───────────────────────────────────────────────────────────
        .route("/health", get(health_check))
}

async fn health_check() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok", "service": "kova-gateway" }))
}

/// Build the full router with all middleware applied.
/// Called from `main.rs` with a live `AppState`.
pub fn build(state: crate::AppState) -> Router {
    use crate::middleware::{
        auth::AuthLayer,
        circuit_breaker::CircuitBreakerLayer,
        rate_limit::{RateLimitConfig, RateLimitLayer},
        tracing::TracingLayer,
    };

    Router::new()
        .nest("/api/v1/kova", routes())
        .layer(TracingLayer::new())
        .layer(RateLimitLayer::new(
            state.redis.clone(),
            RateLimitConfig::default(),
        ))
        .layer(AuthLayer::new(state.jwt_keys.clone()))
        .layer(CircuitBreakerLayer::new())
        // AppState will be used once handlers have state extractor parameters.
        .with_state(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::{Method, Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn make_test_router() -> Router {
        Router::new().nest("/api/v1/kova", routes())
    }

    async fn call(router: Router, method: Method, uri: &str) -> http::Response<axum::body::Body> {
        router
            .oneshot(Request::builder().method(method).uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    macro_rules! assert_route {
        ($router:expr, $method:expr, $path:expr) => {{
            let resp = call($router.clone(), $method, $path).await;
            assert!(
                resp.status() != StatusCode::NOT_FOUND
                    && resp.status() != StatusCode::METHOD_NOT_ALLOWED,
                "route {} {} returned {} — expected a valid route",
                $method,
                $path,
                resp.status()
            );
        }};
    }

    #[tokio::test]
    async fn health_returns_200() {
        let resp = call(make_test_router(), Method::GET, "/api/v1/kova/health").await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn all_routes_exist_and_do_not_panic() {
        let r = make_test_router();
        let id = "01900000-0000-7000-8000-000000000001";

        assert_route!(r, Method::POST,   "/api/v1/kova/auth/register");
        assert_route!(r, Method::POST,   "/api/v1/kova/auth/otp/request");
        assert_route!(r, Method::POST,   "/api/v1/kova/auth/otp/verify");
        assert_route!(r, Method::POST,   "/api/v1/kova/auth/token/refresh");
        assert_route!(r, Method::DELETE, &format!("/api/v1/kova/auth/sessions/{id}"));
        assert_route!(r, Method::POST,   "/api/v1/kova/auth/biometric/challenge");
        assert_route!(r, Method::POST,   "/api/v1/kova/auth/biometric/verify");

        assert_route!(r, Method::POST,   "/api/v1/kova/accounts");
        assert_route!(r, Method::GET,    &format!("/api/v1/kova/accounts/{id}"));
        assert_route!(r, Method::GET,    &format!("/api/v1/kova/accounts/{id}/balance"));
        assert_route!(r, Method::GET,    &format!("/api/v1/kova/accounts/{id}/statement"));

        assert_route!(r, Method::POST,   "/api/v1/kova/payments");
        assert_route!(r, Method::GET,    &format!("/api/v1/kova/payments/{id}"));

        assert_route!(r, Method::POST,   "/api/v1/kova/cards");
        assert_route!(r, Method::GET,    &format!("/api/v1/kova/cards/{id}"));
        assert_route!(r, Method::POST,   &format!("/api/v1/kova/cards/{id}/freeze"));
        assert_route!(r, Method::DELETE, &format!("/api/v1/kova/cards/{id}/freeze"));
        assert_route!(r, Method::GET,    &format!("/api/v1/kova/cards/{id}/transactions"));
        assert_route!(r, Method::PUT,    &format!("/api/v1/kova/cards/{id}/spend-controls"));

        assert_route!(r, Method::POST,   "/api/v1/kova/kyc/submit");
        assert_route!(r, Method::GET,    "/api/v1/kova/kyc/status");

        assert_route!(r, Method::POST,   "/api/v1/kova/fx/quote");
        assert_route!(r, Method::GET,    &format!("/api/v1/kova/fx/quotes/{id}"));
    }

    #[tokio::test]
    async fn unknown_route_returns_404() {
        let resp = call(make_test_router(), Method::GET, "/api/v1/kova/nonexistent").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn stub_routes_return_501_not_implemented() {
        let resp = call(make_test_router(), Method::POST, "/api/v1/kova/auth/register").await;
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "NOT_IMPLEMENTED");
    }
}
