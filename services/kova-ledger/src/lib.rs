pub mod balance;
pub mod error;
pub mod idempotency;
pub mod outbox;
pub mod posting;
pub mod reconciliation;
pub mod server;
pub mod statement;

// gRPC generated code lives in the build-time OUT_DIR.
// pub access lets tests and the server module reference the generated types.
pub mod proto {
    tonic::include_proto!("kova.ledger");
}
