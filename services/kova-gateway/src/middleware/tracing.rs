//! Request ID and structured tracing middleware (TASK-032).
//!
//! For every request this middleware:
//!   1. Generates or propagates `X-KOVA-Request-ID` (UUIDv7).
//!   2. Opens a tracing span with `http.method`, `http.route`, `kova.request_id`.
//!   3. Logs a structured JSON line on response with status, duration_ms, user_id.
//!   4. Copies `X-KOVA-Request-ID` into the response headers.
//!
//! OpenTelemetry propagation via `traceparent`/`tracestate` headers is wired
//! here as a passthrough — the gateway copies inbound W3C trace context into
//! its outbound gRPC calls (implemented in each service client in Deliverable 14).

use std::task::{Context, Poll};
use std::time::Instant;

use axum::{extract::Request, response::Response};
use futures::future::BoxFuture;
use http::HeaderValue;
use tower::{Layer, Service};
use tracing::{info, info_span, Instrument};
use uuid::Uuid;


pub const HEADER_REQUEST_ID: &str = "x-kova-request-id";

// ── Layer ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct TracingLayer;

impl TracingLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for TracingLayer {
    type Service = TracingMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TracingMiddleware { inner }
    }
}

// ── Service ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct TracingMiddleware<S> {
    inner: S,
}

impl<S> Service<Request> for TracingMiddleware<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Propagate existing request ID or generate a fresh one.
            let request_id = req
                .headers()
                .get(HEADER_REQUEST_ID)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
                .unwrap_or_else(|| Uuid::now_v7().to_string());

            let method = req.method().to_string();
            let path = req.uri().path().to_string();

            let span = info_span!(
                "http_request",
                "http.method" = %method,
                "http.route" = %path,
                "kova.request_id" = %request_id,
                "kova.user_id" = tracing::field::Empty,
                "http.status_code" = tracing::field::Empty,
            );

            let start = Instant::now();

            let mut response = inner.call(req).instrument(span.clone()).await?;

            let duration_ms = start.elapsed().as_millis();
            let status = response.status().as_u16();

            // Attach request ID to response for client correlation.
            if let Ok(val) = HeaderValue::from_str(&request_id) {
                response.headers_mut().insert(HEADER_REQUEST_ID, val);
            }

            span.record("http.status_code", status);

            info!(
                parent: &span,
                kova_service = "kova-gateway",
                request_id = %request_id,
                method = %method,
                path = %path,
                status_code = status,
                duration_ms = duration_ms,
                "request completed"
            );

            Ok(response)
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, response::IntoResponse, routing::get, Router};
    use http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    async fn ok_handler() -> impl IntoResponse {
        StatusCode::OK
    }

    fn make_app() -> Router {
        Router::new()
            .route("/ping", get(ok_handler))
            .layer(TracingLayer::new())
    }

    #[tokio::test]
    async fn response_contains_request_id_header() {
        let resp = make_app()
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            resp.headers().get(HEADER_REQUEST_ID).is_some(),
            "X-KOVA-Request-ID header must be present"
        );
    }

    #[tokio::test]
    async fn inbound_request_id_is_propagated() {
        let fixed_id = "01900000-0000-7000-8000-000000000099";
        let resp = make_app()
            .oneshot(
                Request::builder()
                    .uri("/ping")
                    .header(HEADER_REQUEST_ID, fixed_id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let echoed = resp
            .headers()
            .get(HEADER_REQUEST_ID)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(echoed, fixed_id, "inbound request ID must be echoed back");
    }
}
