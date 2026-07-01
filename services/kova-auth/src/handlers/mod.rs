pub mod biometric;
pub mod device;
pub mod health;
pub mod otp;
pub mod pin;
pub mod register;
pub mod token;

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use jsonwebtoken::{Algorithm, Validation};
use serde_json::json;

use crate::{state::AppState, tokens::KovaClaims};

/// Axum extractor that validates the Bearer JWT and returns the decoded claims.
/// Used by endpoints that require an authenticated session.
#[derive(Debug)]
pub struct AuthClaims(pub KovaClaims);

impl FromRequestParts<AppState> for AuthClaims {
    type Rejection = (StatusCode, axum::Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let unauthorized = |msg: &str| {
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({
                    "data": null,
                    "error": { "code": "UNAUTHORIZED", "message": msg },
                    "meta": {}
                })),
            )
        };

        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| unauthorized("Missing Authorization header"))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| unauthorized("Authorization header must be 'Bearer <token>'"))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&["kova-auth"]);

        jsonwebtoken::decode::<KovaClaims>(token, &state.decoding_key, &validation)
            .map(|td| AuthClaims(td.claims))
            .map_err(|_| unauthorized("Invalid or expired access token"))
    }
}
