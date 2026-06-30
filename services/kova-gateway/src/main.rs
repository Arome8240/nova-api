//! kova-gateway binary entrypoint.

mod error;
mod handlers;
mod middleware;
mod router;
mod state;

pub use state::AppState;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Structured JSON logging in production; pretty-print in development.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();

    let jwt_pem = std::env::var("KOVA_JWT_PUBLIC_KEY_PEM")
        .context("KOVA_JWT_PUBLIC_KEY_PEM must be set")?;

    let jwt_keys = middleware::auth::JwtKeySet::from_pem(jwt_pem.as_bytes())
        .context("failed to parse JWT public key")?;

    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
    let redis_client = redis::Client::open(redis_url.as_str())
        .context("failed to create Redis client")?;
    let redis = redis::aio::ConnectionManager::new(redis_client)
        .await
        .context("failed to connect to Redis")?;

    let state = AppState { jwt_keys, redis };

    let addr = std::env::var("KOVA_GATEWAY_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind to {addr}"))?;

    tracing::info!(addr = %addr, "kova-gateway starting");

    axum::serve(listener, router::build(state)).await?;
    Ok(())
}
