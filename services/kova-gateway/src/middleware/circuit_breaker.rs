//! Per-upstream circuit breaker middleware (TASK-033).
//!
//! States:
//!   Closed   — normal operation; tracks consecutive failures.
//!   Open     — fast-fail after 5 consecutive failures; stays open for 30 s.
//!   HalfOpen — allows exactly one probe request after the 30 s timeout.
//!              Success → Closed; failure → Open (resets the 30 s timer).
//!
//! State is per `CircuitBreaker` instance (i.e. per upstream service), not
//! shared across Kubernetes replicas (see TASK-033 notes).

use std::{
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use axum::{extract::Request, response::Response};
use futures::future::BoxFuture;
use tower::{Layer, Service};
use tracing::warn;

use crate::error::ApiError;

// ── State machine ─────────────────────────────────────────────────────────────

const FAILURE_THRESHOLD: u32 = 5;
const OPEN_DURATION: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
enum State {
    Closed { consecutive_failures: u32 },
    Open { opened_at: Instant },
    HalfOpen,
}

impl Default for State {
    fn default() -> Self {
        State::Closed { consecutive_failures: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    service_name: String,
    state: Arc<Mutex<State>>,
}

impl CircuitBreaker {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    /// Returns true if the request is permitted to proceed.
    fn allow_request(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        match &*state {
            State::Closed { .. } => true,
            State::Open { opened_at } => {
                if opened_at.elapsed() >= OPEN_DURATION {
                    // Transition to half-open: allow one probe.
                    *state = State::HalfOpen;
                    true
                } else {
                    false
                }
            }
            State::HalfOpen => false, // only one probe at a time
        }
    }

    fn on_success(&self) {
        let mut state = self.state.lock().unwrap();
        *state = State::Closed { consecutive_failures: 0 };
    }

    fn on_failure(&self) {
        let mut state = self.state.lock().unwrap();
        match *state {
            State::Closed { consecutive_failures } => {
                let next = consecutive_failures + 1;
                if next >= FAILURE_THRESHOLD {
                    warn!(
                        service = %self.service_name,
                        "circuit breaker opened after {next} consecutive failures"
                    );
                    *state = State::Open { opened_at: Instant::now() };
                } else {
                    *state = State::Closed { consecutive_failures: next };
                }
            }
            State::HalfOpen => {
                warn!(
                    service = %self.service_name,
                    "probe failed — circuit remains open"
                );
                *state = State::Open { opened_at: Instant::now() };
            }
            State::Open { .. } => {} // already open
        }
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        matches!(*self.state.lock().unwrap(), State::Open { .. })
    }
}

// ── Tower Layer ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CircuitBreakerLayer {
    breaker: CircuitBreaker,
}

impl CircuitBreakerLayer {
    /// Creates a circuit breaker for the gateway itself (wrapping all routes).
    /// Per-upstream circuit breakers are created in the individual service
    /// client modules (Deliverable 14).
    pub fn new() -> Self {
        Self {
            breaker: CircuitBreaker::new("kova-gateway"),
        }
    }

    #[allow(dead_code)]
    pub fn with_service_name(name: impl Into<String>) -> Self {
        Self {
            breaker: CircuitBreaker::new(name),
        }
    }
}

impl Default for CircuitBreakerLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for CircuitBreakerLayer {
    type Service = CircuitBreakerMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CircuitBreakerMiddleware {
            inner,
            breaker: self.breaker.clone(),
        }
    }
}

// ── Tower Service ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CircuitBreakerMiddleware<S> {
    inner: S,
    breaker: CircuitBreaker,
}

impl<S> Service<Request> for CircuitBreakerMiddleware<S>
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
        let breaker = self.breaker.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            if !breaker.allow_request() {
                return Ok(ApiError::service_unavailable(&breaker.service_name));
            }

            let response = inner.call(req).await?;

            // Treat 5xx as failures for the circuit breaker.
            if response.status().is_server_error() {
                breaker.on_failure();
            } else {
                breaker.on_success();
            }

            Ok(response)
        })
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn breaker() -> CircuitBreaker {
        CircuitBreaker::new("test-service")
    }

    #[test]
    fn closed_by_default() {
        let cb = breaker();
        assert!(cb.allow_request());
        assert!(!cb.is_open());
    }

    #[test]
    fn opens_after_five_failures() {
        let cb = breaker();
        for i in 0..4 {
            cb.on_failure();
            assert!(!cb.is_open(), "should still be closed after {i} failures");
        }
        cb.on_failure(); // 5th failure
        assert!(cb.is_open(), "should open after 5 consecutive failures");
    }

    #[test]
    fn success_resets_failure_count() {
        let cb = breaker();
        cb.on_failure();
        cb.on_failure();
        cb.on_failure();
        cb.on_success();
        // Three more failures should not open the circuit (counter reset).
        cb.on_failure();
        cb.on_failure();
        cb.on_failure();
        assert!(!cb.is_open(), "success should have reset the counter");
    }

    #[test]
    fn open_circuit_blocks_requests() {
        let cb = breaker();
        for _ in 0..5 {
            cb.on_failure();
        }
        assert!(!cb.allow_request(), "open circuit must block requests");
    }

    #[test]
    fn half_open_after_timeout() {
        let cb = CircuitBreaker {
            service_name: "test".to_string(),
            state: Arc::new(Mutex::new(State::Open {
                // Set opened_at in the past so the timeout has elapsed.
                opened_at: Instant::now() - OPEN_DURATION - Duration::from_millis(1),
            })),
        };
        assert!(cb.allow_request(), "should allow probe after timeout");
        // State should now be HalfOpen — second request should be blocked.
        assert!(!cb.allow_request(), "half-open only allows one probe");
    }

    #[test]
    fn probe_failure_reopens_circuit() {
        let cb = CircuitBreaker {
            service_name: "test".to_string(),
            state: Arc::new(Mutex::new(State::HalfOpen)),
        };
        cb.on_failure();
        assert!(cb.is_open(), "probe failure must reopen the circuit");
    }

    #[test]
    fn probe_success_closes_circuit() {
        let cb = CircuitBreaker {
            service_name: "test".to_string(),
            state: Arc::new(Mutex::new(State::HalfOpen)),
        };
        cb.on_success();
        assert!(!cb.is_open(), "probe success must close the circuit");
        assert!(cb.allow_request(), "closed circuit must allow requests");
    }
}
