use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::state::AppState;

/// GET /api/v1/kova/auth/health
///
/// Returns 200 when both MySQL and Redis are reachable, 503 otherwise.
pub async fn health_check(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    let redis_ok = {
        let mut conn = state.redis.clone();
        redis::cmd("PING")
            .query_async::<String>(&mut conn)
            .await
            .map(|r| r == "PONG")
            .unwrap_or(false)
    };

    if db_ok && redis_ok {
        (
            StatusCode::OK,
            Json(json!({
                "data": { "status": "ok", "db": "ok", "redis": "ok" },
                "error": null,
                "meta": {}
            })),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "data": {
                    "status": "degraded",
                    "db": if db_ok { "ok" } else { "down" },
                    "redis": if redis_ok { "ok" } else { "down" }
                },
                "error": { "code": "SERVICE_UNAVAILABLE", "message": "One or more dependencies are unreachable" },
                "meta": {}
            })),
        )
    }
}
