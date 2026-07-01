use std::sync::Arc;

use jsonwebtoken::{DecodingKey, EncodingKey};
use redis::aio::ConnectionManager;
use sqlx::MySqlPool;

use crate::sms::SmsProvider;

#[derive(Clone)]
pub struct AppState {
    pub db: MySqlPool,
    pub redis: ConnectionManager,
    pub encoding_key: EncodingKey,
    pub decoding_key: DecodingKey,
    pub sms: Arc<dyn SmsProvider>,
}

impl AppState {
    pub fn new(
        db: MySqlPool,
        redis: ConnectionManager,
        private_pem: &[u8],
        public_pem: &[u8],
        sms: Arc<dyn SmsProvider>,
    ) -> Result<Self, jsonwebtoken::errors::Error> {
        Ok(Self {
            db,
            redis,
            encoding_key: EncodingKey::from_rsa_pem(private_pem)?,
            decoding_key: DecodingKey::from_rsa_pem(public_pem)?,
            sms,
        })
    }
}
