//! Cryptographic primitives for PIN hashing, OTP generation, and OTP hashing.
//!
//! PIN:  argon2id — Argon2id, 64 MB memory, 3 iterations, parallelism 4.
//! OTP:  6-digit code from `ring::rand::SystemRandom` (rejection-sampled),
//!       stored hashed with bcrypt cost 10.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Params,
};
use ring::rand::{SecureRandom, SystemRandom};

use crate::error::AuthError;

// OWASP-recommended Argon2id parameters (2024)
const ARGON2_MEM_KB: u32 = 65_536;   // 64 MB
const ARGON2_ITER: u32 = 3;
const ARGON2_PAR: u32 = 4;

// Rejection-sampling threshold: largest multiple of 1_000_000 that fits in u32.
// floor(2^32 / 1_000_000) * 1_000_000 = 4_294_000_000.
// Any n ≥ 4_294_000_000 is rejected (probability ≈ 0.023% per draw).
const OTP_REJECT_THRESHOLD: u32 = 4_294_000_000;

fn argon2() -> Result<Argon2<'static>, AuthError> {
    let params = Params::new(ARGON2_MEM_KB, ARGON2_ITER, ARGON2_PAR, None)
        .map_err(|e| AuthError::Crypto(e.to_string()))?;
    Ok(Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params))
}

/// Hash a PIN with Argon2id. Each call produces a unique salt.
///
/// `tracing::instrument` skips the pin value so it never appears in span fields.
#[tracing::instrument(skip(pin), fields(op = "hash_pin"))]
pub fn hash_pin(pin: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    argon2()?
        .hash_password(pin.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Crypto(e.to_string()))
}

/// Verify a PIN against its stored Argon2id hash in constant time.
///
/// Returns `Ok(false)` if `hash` is empty (PIN not yet set).
#[tracing::instrument(skip(pin, hash), fields(op = "verify_pin"))]
pub fn verify_pin(pin: &str, hash: &str) -> Result<bool, AuthError> {
    if hash.is_empty() {
        return Ok(false);
    }
    let parsed = PasswordHash::new(hash).map_err(|e| AuthError::Crypto(e.to_string()))?;
    Ok(argon2()?.verify_password(pin.as_bytes(), &parsed).is_ok())
}

/// Generate a cryptographically random 6-digit OTP (uniformly distributed,
/// no modulo bias via rejection sampling).
pub fn generate_otp() -> Result<String, AuthError> {
    let rng = SystemRandom::new();
    loop {
        let mut buf = [0u8; 4];
        rng.fill(&mut buf).map_err(|_| AuthError::Crypto("rng fill failed".into()))?;
        let n = u32::from_be_bytes(buf);
        if n < OTP_REJECT_THRESHOLD {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}

/// Hash an OTP with bcrypt (cost 10). Fast enough for the 50 ms SLA.
pub fn hash_otp(otp: &str) -> Result<String, AuthError> {
    bcrypt::hash(otp, 10).map_err(|e| AuthError::Crypto(e.to_string()))
}

/// Verify an OTP against its bcrypt hash.
pub fn verify_otp(otp: &str, hash: &str) -> Result<bool, AuthError> {
    bcrypt::verify(otp, hash).map_err(|e| AuthError::Crypto(e.to_string()))
}

/// Generate 32 cryptographically random bytes and return as a 64-char hex string.
/// Used for refresh tokens and biometric challenge tokens.
pub fn random_hex_token() -> Result<String, AuthError> {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes).map_err(|_| AuthError::Crypto("rng fill failed".into()))?;
    Ok(hex::encode(bytes))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_pin_hashes_differ() {
        let h1 = hash_pin("1234").unwrap();
        let h2 = hash_pin("1234").unwrap();
        assert_ne!(h1, h2, "different salts must produce different hashes");
        assert!(verify_pin("1234", &h1).unwrap());
        assert!(verify_pin("1234", &h2).unwrap());
    }

    #[test]
    fn wrong_pin_fails_verification() {
        let hash = hash_pin("1234").unwrap();
        assert!(!verify_pin("9999", &hash).unwrap());
    }

    #[test]
    fn empty_hash_means_pin_not_set() {
        assert!(!verify_pin("1234", "").unwrap());
    }

    #[test]
    fn otp_is_six_digits() {
        for _ in 0..50 {
            let otp = generate_otp().unwrap();
            assert_eq!(otp.len(), 6, "OTP must be exactly 6 chars: {otp}");
            assert!(otp.chars().all(|c| c.is_ascii_digit()), "OTP must be numeric: {otp}");
        }
    }

    #[test]
    fn otp_hash_verifies() {
        let otp = generate_otp().unwrap();
        let hash = hash_otp(&otp).unwrap();
        assert!(verify_otp(&otp, &hash).unwrap());
        let wrong = format!("{:06}", (otp.parse::<u32>().unwrap() + 1) % 1_000_000);
        assert!(!verify_otp(&wrong, &hash).unwrap());
    }

    #[test]
    fn random_hex_token_is_64_chars() {
        let t = random_hex_token().unwrap();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
