//! Startup and dependency health routes only. Authentication routes belong to later tasks.
pub mod accounts;
pub mod mfa;
pub mod passwords;
pub mod security;
pub mod sessions;
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use identity_store::Dependencies;
use serde_json::{Value, json};

pub fn router(dependencies: Dependencies) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .with_state(dependencies)
}

async fn live() -> Json<Value> {
    Json(json!({ "status": "live" }))
}

async fn ready(State(dependencies): State<Dependencies>) -> (StatusCode, Json<Value>) {
    if dependencies.ready().await {
        (StatusCode::OK, Json(json!({ "status": "ready" })))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "unavailable" })),
        )
    }
}

/// Production exposes only implemented health and durable preauthentication routes.
pub fn secured_router(dependencies: Dependencies, state: security::SecurityState) -> Router {
    security::security_router(state, router(dependencies))
}

/// Health plus actually implemented account routes with the common security boundary.
pub fn application_router(dependencies: Dependencies, state: accounts::AuthAppState) -> Router {
    accounts::accounts_router(state, router(dependencies))
}
