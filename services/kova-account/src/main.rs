use std::sync::Arc;

use anyhow::Context;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use kova_account::{
    clients::{LedgerStub, NoOpPublisher, PaymentsStub},
    router,
    state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().json())
        .init();

    let database_url  = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let redis_url     = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let jwt_public_key = std::env::var("JWT_PUBLIC_KEY").expect("JWT_PUBLIC_KEY must be set");
    let bind_addr     = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3002".to_string());

    let db = sqlx::MySqlPool::connect(&database_url)
        .await
        .context("failed to connect to MySQL")?;

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .context("failed to run database migrations")?;

    let redis_client = redis::Client::open(redis_url.as_str())
        .context("failed to parse REDIS_URL")?;
    let redis = redis::aio::ConnectionManager::new(redis_client)
        .await
        .context("failed to connect to Redis")?;

    let state = AppState::new(
        db,
        redis,
        jwt_public_key.as_bytes(),
        Arc::new(LedgerStub),
        Arc::new(PaymentsStub),
        Arc::new(NoOpPublisher),
    )
    .context("failed to initialise JWT key set")?;

    let app = router::build(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("failed to bind to {bind_addr}"))?;

    tracing::info!("kova-account listening on {}", listener.local_addr()?);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal;
    let ctrl_c = async { signal::ctrl_c().await.expect("Ctrl+C handler failed") };

    #[cfg(unix)]
    let sigterm = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler failed")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c  => tracing::info!("received Ctrl+C"),
        _ = sigterm => tracing::info!("received SIGTERM"),
    }
}
