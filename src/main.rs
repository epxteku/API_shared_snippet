#![allow(non_snake_case)]
use std::sync::Arc;
use tokio::task;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::{TraceLayer, DefaultMakeSpan};
use tracing::{info, Level, Span};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use axum::{Server, Router, extract::Extension};
use axum::http::Request;
use api::create_api_routes;
use load_resources::{create_app_state, reload_tokens};
use path_updater::start_all_update_processes;
use std::env;
use std::fs::File;
use pprof::ProfilerGuardBuilder;

pub mod api;
pub mod load_resources;
pub mod paths;
pub mod services;
pub mod utils;
pub mod dapps;
pub mod create_clients;
pub mod path_updater;

#[tokio::main]
async fn main() {
    // Start pprof profiling
    let guard = ProfilerGuardBuilder::default()
        .frequency(100)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .unwrap();

    // Create a custom EnvFilter
    let filter = EnvFilter::new("off");

    // Initialize tracing for logging
    tracing_subscriber::registry()
        .with(fmt::layer()
            .with_target(true)
            .pretty())
        .with(filter)
        .init();

    // Create the AppState with dapps, chains, tokens, and proxy clients loaded from JSON files
    let state = Arc::new(create_app_state().await);

    // Spawn a background task to reload the tokens.json every 5 minutes
    let state_clone = Arc::clone(&state);
    task::spawn(async move {
        reload_tokens(state_clone).await;
    });

    /*/ Spawn a background task to start the update processes without blocking the main API
    let state_clone = Arc::clone(&state);
    task::spawn(async move {
        if let Err(e) = start_all_update_processes(state_clone).await {
            eprintln!("Error in start_all_update_processes: {:?}", e);
        }
    }); */
    
    // Create all routes by calling `create_api_routes`
    let app = Router::new()
        .merge(create_api_routes(state))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http()
                    .on_request(|request: &Request<_>, _span: &Span| {
                        tracing::info!(
                            "Received a request: {} {}",
                            request.method(),
                            request.uri().path()
                        );
                    })
                    .make_span_with(DefaultMakeSpan::new()
                        .level(Level::INFO)
                    )
                )
        )
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    // Start the server
    let addr = "0.0.0.0:3000".parse().unwrap();
    info!("Server running on http://{}", addr);
    
    // Run the server
    let server = Server::bind(&addr).serve(app.into_make_service());
    
    // Use tokio::select to run the server and handle a shutdown signal
    tokio::select! {
        _ = server => {},
        _ = tokio::signal::ctrl_c() => {
            println!("Received Ctrl+C, shutting down");
        }
    }

    // Generate pprof report
    if let Ok(report) = guard.report().build() {
        let file = File::create("flamegraph.svg").unwrap();
        report.flamegraph(file).unwrap();
        println!("Flamegraph generated: flamegraph.svg");
    }
}