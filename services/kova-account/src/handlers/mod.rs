pub mod balance;
pub mod close;
pub mod create;
pub mod get;
pub mod health;
pub mod statement;

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use jsonwebtoken::{Algorithm, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::state::AppState;

/// Decoded JWT claims injected by the `AuthUser` extractor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub sub: String,
    pub jti: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = (StatusCode, axum::Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let err = |msg: &str| {
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({
                    "data": null,
                    "error": { "code": "UNAUTHORIZED", "message": msg },
                    "meta": {}
                })),
            )
        };

        let header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| err("Missing Authorization header"))?;

        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| err("Authorization header must be 'Bearer <token>'"))?;

        #[derive(Debug, Deserialize)]
        struct Claims {
            sub: String,
            jti: String,
        }

        let mut val = Validation::new(Algorithm::RS256);
        val.set_issuer(&["kova-auth"]);

        jsonwebtoken::decode::<Claims>(token, &state.decoding_key, &val)
            .map(|td| AuthUser { sub: td.claims.sub, jti: td.claims.jti })
            .map_err(|_| err("Invalid or expired access token"))
    }
}
