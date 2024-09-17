//src/api.rs
use axum::Router;
use std::sync::Arc;
use crate::load_resources::AppState;
use crate::paths::resources::create_resource_routes;
use crate::paths::quote::create_quote_routes;
use crate::paths::quote_stream::create_quote_stream_routes;
use crate::paths::quote_direct::create_quote_direct_routes;
use crate::paths::build_transaction::create_build_transaction_routes;

pub fn create_api_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(create_resource_routes(Arc::clone(&state)))
        .merge(create_quote_routes(Arc::clone(&state)))
        .merge(create_quote_stream_routes(Arc::clone(&state)))
        .merge(create_quote_direct_routes(Arc::clone(&state)))
        .merge(create_build_transaction_routes(Arc::clone(&state)))
}