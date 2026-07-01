use std::sync::Arc;

use jsonwebtoken::DecodingKey;
use redis::aio::ConnectionManager;
use sqlx::MySqlPool;

use crate::clients::{EventPublisher, LedgerClient, PaymentsClient};

#[derive(Clone)]
pub struct AppState {
    pub db: MySqlPool,
    pub redis: ConnectionManager,
    pub decoding_key: DecodingKey,
    pub ledger: Arc<dyn LedgerClient>,
    pub payments: Arc<dyn PaymentsClient>,
    pub events: Arc<dyn EventPublisher>,
}

impl AppState {
    pub fn new(
        db: MySqlPool,
        redis: ConnectionManager,
        public_pem: &[u8],
        ledger: Arc<dyn LedgerClient>,
        payments: Arc<dyn PaymentsClient>,
        events: Arc<dyn EventPublisher>,
    ) -> Result<Self, jsonwebtoken::errors::Error> {
        Ok(Self {
            db,
            redis,
            decoding_key: DecodingKey::from_rsa_pem(public_pem)?,
            ledger,
            payments,
            events,
        })
    }
}
