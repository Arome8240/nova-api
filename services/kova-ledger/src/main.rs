//! TASK-057 — kova-ledger gRPC server entry point.
//!
//! Starts three concurrent tasks:
//!   1. The tonic gRPC server (PostEntry / GetBalance / GetStatement).
//!   2. The outbox worker — polls `ledger_outbox` and publishes to Kafka
//!      (or the no-op publisher when the `kafka` feature is disabled).
//!   3. The EOD reconciliation scheduler — runs a balance-vs-snapshot check
//!      nightly in the 23:55–00:05 UTC window.
//!
//! Environment variables (all required unless noted):
//!   DATABASE_URL    — MySQL DSN for kova_ledger_db
//!   BIND_ADDR       — gRPC listen address, default "[::1]:50051"
//!   JWT_PUBLIC_KEY  — RSA PEM public key issued by kova-auth
//!   KAFKA_BROKERS   — comma-separated broker list (only with `--features kafka`)
//!
//! The gRPC port must NOT be exposed outside the Kubernetes cluster
//! (ClusterIP service only — see TASKS.md security constraints).

use std::sync::Arc;

use anyhow::Context;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use kova_ledger::{
    outbox::{run_outbox_worker, NoOpPublisher},
    proto::kova_ledger_server::KovaLedgerServer,
    reconciliation::run_eod_reconciliation,
    server::{make_auth_interceptor, make_service},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Structured logging ────────────────────────────────────────────────────
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().json())
        .init();

    // ── Required environment variables ────────────────────────────────────────
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let bind_addr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "[::1]:50051".to_string());
    let jwt_public_key =
        std::env::var("JWT_PUBLIC_KEY").expect("JWT_PUBLIC_KEY must be set");

    // ── Database pool ─────────────────────────────────────────────────────────
    let pool = sqlx::MySqlPool::connect(&database_url)
        .await
        .context("failed to connect to MySQL")?;

    tracing::info!("connected to MySQL");

    // ── Outbox publisher ──────────────────────────────────────────────────────
    #[cfg(feature = "kafka")]
    let publisher: Arc<dyn kova_ledger::outbox::OutboxPublisher> = {
        let brokers = std::env::var("KAFKA_BROKERS").expect("KAFKA_BROKERS must be set with --features kafka");
        let producer = kova_kafka::KovaKafkaProducer::new(&brokers)
            .context("failed to create Kafka producer")?;
        Arc::new(kova_ledger::outbox::KafkaPublisher(Arc::new(producer)))
    };
    #[cfg(not(feature = "kafka"))]
    let publisher: Arc<dyn kova_ledger::outbox::OutboxPublisher> = Arc::new(NoOpPublisher);

    // ── Spawn background tasks ────────────────────────────────────────────────
    let outbox_pool = pool.clone();
    tokio::spawn(run_outbox_worker(outbox_pool, publisher));

    let reconciliation_pool = pool.clone();
    tokio::spawn(run_eod_reconciliation(reconciliation_pool));

    // ── gRPC server ───────────────────────────────────────────────────────────
    let interceptor = make_auth_interceptor(jwt_public_key.as_bytes())
        .context("failed to build auth interceptor")?;
    let grpc_service = KovaLedgerServer::with_interceptor(make_service(pool), interceptor);

    let addr = bind_addr
        .parse()
        .with_context(|| format!("invalid BIND_ADDR: {bind_addr}"))?;

    tracing::info!(%addr, "kova-ledger gRPC server starting");

    tonic::transport::Server::builder()
        .add_service(grpc_service)
        .serve_with_shutdown(addr, shutdown_signal())
        .await
        .context("gRPC server error")?;

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
        _ = ctrl_c  => tracing::info!("received Ctrl+C, shutting down"),
        _ = sigterm => tracing::info!("received SIGTERM, shutting down"),
    }
}
