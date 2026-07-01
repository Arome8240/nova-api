use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::{
    handlers::{
        balance::get_balance,
        close::close_account,
        create::create_account,
        get::{get_account, list_accounts},
        health::health_check,
        statement::get_statement,
    },
    state::AppState,
};

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/kova/account/health",          get(health_check))
        .route("/api/v1/kova/accounts",                post(create_account))
        .route("/api/v1/kova/accounts",                get(list_accounts))
        .route("/api/v1/kova/accounts/{id}",           get(get_account))
        .route("/api/v1/kova/accounts/{id}",           delete(close_account))
        .route("/api/v1/kova/accounts/{id}/balance",   get(get_balance))
        .route("/api/v1/kova/accounts/{id}/statement", get(get_statement))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
