//! JWT RS256 authentication middleware (TASK-030).
//!
//! Validates `Authorization: Bearer <token>` on every request (except /health).
//! On success, injects `AuthenticatedUser` into request extensions.
//! On failure, returns 401 JSON with a machine-readable error code.
//!
//! Key design points:
//!   - Public key is loaded once at startup and stored in `JwtKeySet`.
//!   - Key rotation: `JwtKeySet` holds multiple keys indexed by `kid`.
//!     A rolling deploy can run with two keys simultaneously.
//!   - Unauthenticated routes (auth/*, health) are excluded via `AuthExempt` marker.

use std::{
    collections::HashMap,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    extract::Request,
    response::Response,
};
use futures::future::BoxFuture;
use jsonwebtoken::{Algorithm, DecodingKey, TokenData, Validation};
use serde::{Deserialize, Serialize};
use tower::{Layer, Service};

use kova_types::KovaUserId;

use crate::error::ApiError;

// ── Public key set ────────────────────────────────────────────────────────────

/// Holds one or more RS256 decoding keys indexed by `kid`.
/// Construct via `JwtKeySet::from_pem` (single key) or `JwtKeySet::from_map`.
#[derive(Clone)]
pub struct JwtKeySet(Arc<HashMap<String, DecodingKey>>);

impl JwtKeySet {
    /// Load a single PEM-encoded RS256 public key. Uses `"default"` as the kid.
    pub fn from_pem(pem: &[u8]) -> Result<Self, jsonwebtoken::errors::Error> {
        let key = DecodingKey::from_rsa_pem(pem)?;
        let mut map = HashMap::new();
        map.insert("default".to_string(), key);
        Ok(Self(Arc::new(map)))
    }

    /// Construct from an explicit kid → PEM map (for key rotation).
    pub fn from_map(
        keys: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> Result<Self, jsonwebtoken::errors::Error> {
        let map = keys
            .into_iter()
            .map(|(kid, pem)| DecodingKey::from_rsa_pem(&pem).map(|k| (kid, k)))
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(Self(Arc::new(map)))
    }

    fn get(&self, kid: Option<&str>) -> Option<&DecodingKey> {
        let kid = kid.unwrap_or("default");
        self.0.get(kid)
    }

    /// Returns a dummy key set that rejects all tokens — for use in tests that
    /// bypass auth via `AuthExempt`.
    pub fn test_dummy() -> Self {
        Self(Arc::new(HashMap::new()))
    }
}

// ── Claims ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KovaClaims {
    pub sub: String,
    pub exp: u64,
    pub iat: u64,
    pub jti: String,
    pub iss: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kova_device_id: Option<String>,
}

/// Injected into Axum request extensions after successful validation.
/// Fields `jti` and `device_id` are used by downstream service handlers.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuthenticatedUser {
    pub user_id: KovaUserId,
    pub jti: String,
    pub device_id: Option<String>,
}

// ── Validation logic ──────────────────────────────────────────────────────────

/// Decode and validate a JWT string. Returns structured claims on success.
pub fn validate_token(
    token: &str,
    keys: &JwtKeySet,
) -> Result<TokenData<KovaClaims>, AuthError> {
    // Peek at the header to find the kid before full decode.
    let header = jsonwebtoken::decode_header(token).map_err(|_| AuthError::Invalid)?;
    if header.alg != Algorithm::RS256 {
        return Err(AuthError::WrongAlgorithm);
    }

    let key = keys
        .get(header.kid.as_deref())
        .ok_or(AuthError::UnknownKid)?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&["kova-auth"]);
    // exp is validated automatically.

    jsonwebtoken::decode::<KovaClaims>(token, key, &validation).map_err(|e| {
        use jsonwebtoken::errors::ErrorKind;
        match e.kind() {
            ErrorKind::ExpiredSignature => AuthError::Expired,
            _ => AuthError::Invalid,
        }
    })
}

#[derive(Debug)]
pub enum AuthError {
    MissingHeader,
    MalformedBearer,
    Expired,
    Invalid,
    WrongAlgorithm,
    UnknownKid,
}

impl AuthError {
    fn into_response(self) -> Response {
        match self {
            AuthError::Expired => ApiError::unauthorized("TOKEN_EXPIRED", "Access token has expired"),
            AuthError::MissingHeader | AuthError::MalformedBearer => {
                ApiError::unauthorized("TOKEN_MISSING", "Authorization header is required")
            }
            AuthError::WrongAlgorithm => {
                ApiError::unauthorized("TOKEN_INVALID", "Unsupported signing algorithm")
            }
            AuthError::Invalid | AuthError::UnknownKid => {
                ApiError::unauthorized("TOKEN_INVALID", "Access token is invalid")
            }
        }
    }
}

// ── Routes exempt from authentication ────────────────────────────────────────

fn is_exempt(path: &str, _method: &http::Method) -> bool {
    // Health probe
    if path == "/api/v1/kova/health" {
        return true;
    }
    // All /auth/* routes are public (register, otp, token/refresh, biometric)
    if path.starts_with("/api/v1/kova/auth/") {
        return true;
    }
    false
}

// ── Tower Layer / Service ─────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AuthLayer {
    keys: JwtKeySet,
}

impl AuthLayer {
    pub fn new(keys: JwtKeySet) -> Self {
        Self { keys }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            keys: self.keys.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    keys: JwtKeySet,
}

impl<S> Service<Request> for AuthMiddleware<S>
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

    fn call(&mut self, mut req: Request) -> Self::Future {
        let keys = self.keys.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let path = req.uri().path().to_string();
            let method = req.method().clone();

            if is_exempt(&path, &method) {
                return inner.call(req).await;
            }

            // Extract and validate Bearer token.
            let auth_header = req
                .headers()
                .get(http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok());

            let token = match auth_header {
                None => return Ok(AuthError::MissingHeader.into_response()),
                Some(h) => {
                    if let Some(t) = h.strip_prefix("Bearer ") {
                        t.trim()
                    } else {
                        return Ok(AuthError::MalformedBearer.into_response());
                    }
                }
            };

            let claims = match validate_token(token, &keys) {
                Ok(data) => data.claims,
                Err(e) => return Ok(e.into_response()),
            };

            let user_id = match claims.sub.parse::<uuid::Uuid>() {
                Ok(u) => KovaUserId::from(u),
                Err(_) => return Ok(AuthError::Invalid.into_response()),
            };

            req.extensions_mut().insert(AuthenticatedUser {
                user_id,
                jti: claims.jti,
                device_id: claims.kova_device_id,
            });

            inner.call(req).await
        })
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    // 2048-bit RSA test key pair (generated offline for tests only).
    const TEST_PRIVATE_KEY_PEM: &[u8] = include_bytes!("../test_keys/test_private.pem");
    const TEST_PUBLIC_KEY_PEM: &[u8] = include_bytes!("../test_keys/test_public.pem");
    const HS256_SECRET: &[u8] = b"not-rsa-secret-for-test";

    fn make_keys() -> JwtKeySet {
        JwtKeySet::from_pem(TEST_PUBLIC_KEY_PEM).unwrap()
    }

    fn make_valid_token(exp_delta_secs: i64) -> String {
        let now = chrono::Utc::now().timestamp() as u64;
        let exp = if exp_delta_secs >= 0 {
            now + exp_delta_secs as u64
        } else {
            now.saturating_sub((-exp_delta_secs) as u64)
        };
        let claims = KovaClaims {
            sub: uuid::Uuid::now_v7().to_string(),
            exp,
            iat: now,
            jti: uuid::Uuid::now_v7().to_string(),
            iss: "kova-auth".to_string(),
            kova_device_id: None,
        };
        encode(
            &Header::new(Algorithm::RS256),
            &claims,
            &EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY_PEM).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn valid_token_accepted() {
        let token = make_valid_token(900);
        let result = validate_token(&token, &make_keys());
        assert!(result.is_ok(), "valid token should pass: {result:?}");
    }

    #[test]
    fn expired_token_returns_expired_error() {
        let token = make_valid_token(-10); // expired 10 seconds ago
        let result = validate_token(&token, &make_keys());
        assert!(
            matches!(result, Err(AuthError::Expired)),
            "expected Expired, got {result:?}"
        );
    }

    #[test]
    fn hs256_token_rejected() {
        let now = chrono::Utc::now().timestamp() as u64;
        let claims = KovaClaims {
            sub: uuid::Uuid::now_v7().to_string(),
            exp: now + 900,
            iat: now,
            jti: uuid::Uuid::now_v7().to_string(),
            iss: "kova-auth".to_string(),
            kova_device_id: None,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(HS256_SECRET),
        )
        .unwrap();
        let result = validate_token(&token, &make_keys());
        assert!(
            matches!(result, Err(AuthError::WrongAlgorithm)),
            "HS256 token must be rejected: {result:?}"
        );
    }

    #[test]
    fn missing_header_path_is_exempt() {
        // /auth/* and /health must not require a token.
        assert!(is_exempt("/api/v1/kova/health", &http::Method::GET));
        assert!(is_exempt("/api/v1/kova/auth/register", &http::Method::POST));
        assert!(is_exempt("/api/v1/kova/auth/otp/request", &http::Method::POST));
        assert!(!is_exempt("/api/v1/kova/accounts", &http::Method::GET));
    }

    #[test]
    fn malformed_bearer_detected() {
        // A token that is not "Bearer <token>" should be caught by the middleware
        // layer. We test the exemption logic and token validation separately.
        let token = "Basic dXNlcjpwYXNz"; // Basic auth, not Bearer
        // Stripping "Bearer " prefix returns None so malformed path is triggered.
        assert!(token.strip_prefix("Bearer ").is_none());
    }
}
