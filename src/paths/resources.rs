//src/paths/resources.rs
use axum::{Json, Router, routing::get, http::StatusCode};
use std::sync::Arc;
use tracing::info;
use crate::load_resources::AppState;

// Define the GET /api/dapps route
pub async fn get_dapps(state: Arc<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Received GET request for /api/dapps");
    Ok(Json(state.dapps.clone()))
}

// Define the GET /api/chains route
pub async fn get_chains(state: Arc<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Received GET request for /api/chains");
    Ok(Json(state.chains.clone()))
}

// Define the GET /api/tokens route
pub async fn get_tokens(state: Arc<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    info!("Received GET request for /api/tokens");
    let tokens = state.tokens.get("tokens").map(|v| v.clone());
    match tokens {
        Some(value) => Ok(Json(value)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

// Create a router for resource-related routes
pub fn create_resource_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/dapps", get({
            let state = Arc::clone(&state);
            move || get_dapps(state)
        }))
        .route("/api/chains", get({
            let state = Arc::clone(&state);
            move || get_chains(state)
        }))
        .route("/api/tokens", get({
            let state = Arc::clone(&state);
            move || get_tokens(state)
        }))
}
