//! Redis sliding-window rate limiter (TASK-031).
//!
//! Three tiers:
//!   - `Auth`     — 10 req/min per IP  (OTP endpoints, unauthenticated)
//!   - `Standard` — 100 req/min per user_id
//!   - `Payment`  — 30 req/min per user_id (payment initiation only)
//!
//! Key format: `kova:ratelimit:<tier>:<identifier>:<window_minute>`
//!
//! The Lua script is atomic — no race condition between check and increment.
//! TTL is set to window_secs + 1 so keys expire naturally.

use std::task::{Context, Poll};

use axum::{extract::Request, response::Response};
use futures::future::BoxFuture;
use redis::aio::ConnectionManager;
use tower::{Layer, Service};

use crate::{
    error::ApiError,
    middleware::auth::AuthenticatedUser,
};

// ── Tier configuration ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Auth,
    Standard,
    Payment,
}

impl Tier {
    fn name(self) -> &'static str {
        match self {
            Tier::Auth => "auth",
            Tier::Standard => "standard",
            Tier::Payment => "payment",
        }
    }

    fn limit(self) -> u64 {
        match self {
            Tier::Auth => 10,
            Tier::Standard => 100,
            Tier::Payment => 30,
        }
    }

    fn window_secs(self) -> u64 {
        60 // all tiers use a 60-second window
    }
}

#[derive(Clone)]
pub struct RateLimitConfig {
    /// Which tier applies to each route prefix.
    pub auth_prefix: &'static str,
    pub payment_prefix: &'static str,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            auth_prefix: "/api/v1/kova/auth/otp",
            payment_prefix: "/api/v1/kova/payments",
        }
    }
}

impl RateLimitConfig {
    fn tier_for(&self, path: &str) -> Tier {
        if path.starts_with(self.auth_prefix) {
            Tier::Auth
        } else if path.starts_with(self.payment_prefix) {
            Tier::Payment
        } else {
            Tier::Standard
        }
    }
}

// ── Lua script (atomic sliding-window increment + check) ─────────────────────

/// KEYS[1] = redis key, ARGV[1] = limit, ARGV[2] = TTL seconds
/// Returns 1 if allowed, 0 if limit exceeded. Sets TTL on first use.
const LUA_SLIDING_WINDOW: &str = r#"
local key   = KEYS[1]
local limit = tonumber(ARGV[1])
local ttl   = tonumber(ARGV[2])
local count = redis.call('INCR', key)
if count == 1 then
    redis.call('EXPIRE', key, ttl)
end
if count > limit then
    return 0
end
return 1
"#;

// ── Tower Layer / Service ─────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RateLimitLayer {
    redis: ConnectionManager,
    config: RateLimitConfig,
}

impl RateLimitLayer {
    pub fn new(redis: ConnectionManager, config: RateLimitConfig) -> Self {
        Self { redis, config }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitMiddleware {
            inner,
            redis: self.redis.clone(),
            config: self.config.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitMiddleware<S> {
    inner: S,
    redis: ConnectionManager,
    config: RateLimitConfig,
}

impl<S> Service<Request> for RateLimitMiddleware<S>
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
        let mut redis = self.redis.clone();
        let config = self.config.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let path = req.uri().path().to_string();
            let tier = config.tier_for(&path);

            // Skip rate limiting for health checks.
            if path == "/api/v1/kova/health" {
                return inner.call(req).await;
            }

            // Choose identifier: IP for Auth tier, user_id for others.
            let identifier = if tier == Tier::Auth {
                extract_client_ip(&req)
            } else {
                req.extensions()
                    .get::<AuthenticatedUser>()
                    .map(|u| u.user_id.to_string())
                    .unwrap_or_else(|| extract_client_ip(&req))
            };

            let window_minute = chrono::Utc::now().timestamp() / 60;
            let redis_key = format!(
                "kova:ratelimit:{}:{}:{}",
                tier.name(),
                identifier,
                window_minute
            );
            let ttl = tier.window_secs() + 1;

            let allowed: i64 = match redis::Script::new(LUA_SLIDING_WINDOW)
                .key(&redis_key)
                .arg(tier.limit())
                .arg(ttl)
                .invoke_async(&mut redis)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    // Redis failure: fail open (don't block legitimate traffic).
                    tracing::warn!(error = %e, "redis rate limit error — failing open");
                    1
                }
            };

            if allowed == 0 {
                // Seconds until the next minute window.
                let now_secs = chrono::Utc::now().timestamp() as u64;
                let retry_after = 60 - (now_secs % 60);
                return Ok(ApiError::too_many_requests(retry_after));
            }

            inner.call(req).await
        })
    }
}

/// Extract a best-effort client IP. Trusts X-Forwarded-For only when running
/// behind the cluster ingress (trust is controlled by deployment, not code).
fn extract_client_ip(req: &Request) -> String {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|ip| ip.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_for_otp_is_auth() {
        let cfg = RateLimitConfig::default();
        assert_eq!(cfg.tier_for("/api/v1/kova/auth/otp/request"), Tier::Auth);
        assert_eq!(cfg.tier_for("/api/v1/kova/auth/otp/verify"), Tier::Auth);
    }

    #[test]
    fn tier_for_payment_is_payment() {
        let cfg = RateLimitConfig::default();
        assert_eq!(cfg.tier_for("/api/v1/kova/payments"), Tier::Payment);
        assert_eq!(cfg.tier_for("/api/v1/kova/payments/some-id"), Tier::Payment);
    }

    #[test]
    fn tier_for_accounts_is_standard() {
        let cfg = RateLimitConfig::default();
        assert_eq!(cfg.tier_for("/api/v1/kova/accounts"), Tier::Standard);
        assert_eq!(cfg.tier_for("/api/v1/kova/cards"), Tier::Standard);
    }

    #[test]
    fn tier_limits() {
        assert_eq!(Tier::Auth.limit(), 10);
        assert_eq!(Tier::Standard.limit(), 100);
        assert_eq!(Tier::Payment.limit(), 30);
    }

    #[test]
    fn redis_key_format() {
        let key = format!("kova:ratelimit:{}:{}:{}", Tier::Auth.name(), "1.2.3.4", 12345);
        assert_eq!(key, "kova:ratelimit:auth:1.2.3.4:12345");
    }
}
