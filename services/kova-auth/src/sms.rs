use async_trait::async_trait;

use crate::error::AuthError;

#[async_trait]
pub trait SmsProvider: Send + Sync {
    async fn send_otp(&self, phone: &str, otp: &str) -> Result<(), AuthError>;
}

/// Development/test mock — logs the OTP to stdout in debug builds only.
pub struct MockSmsProvider;

#[async_trait]
impl SmsProvider for MockSmsProvider {
    async fn send_otp(&self, phone: &str, otp: &str) -> Result<(), AuthError> {
        // Guard: OTP must never appear in logs in production builds.
        if cfg!(debug_assertions) {
            tracing::info!(phone = %phone, otp = %otp, "SMS OTP (dev-only)");
        } else {
            tracing::info!(phone = %phone, "OTP SMS dispatched");
        }
        Ok(())
    }
}
