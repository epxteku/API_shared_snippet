//src/services/quote_service.rs
use std::sync::Arc;
use dashmap::DashMap;
use serde_json::{Value, json};
use crate::services::quote_router::route_quote;
use crate::services::quote_stream_router::route_quote_stream;
use crate::load_resources::AppState;
use tokio::time::{Duration, sleep};
use uuid::Uuid;
use tokio::sync::mpsc;
use tracing::{error, info};
use crate::services::transaction_router::route_transaction_from_quote;

type Cache = Arc<DashMap<String, Value>>;

pub async fn process_quote(params: Value, cache: Cache, state: Arc<AppState>) -> Result<Value, String> {
    info!("Processing quote with params: {:?}", params);

    // Process the quote in a separate task to avoid blocking
    let quote_result = tokio::task::spawn(route_quote(params, Arc::clone(&state))).await
        .map_err(|e| format!("Quote task panicked: {}", e))?;

    match quote_result {
        Ok(response) => {
            let request_id = Uuid::new_v4().to_string();
            
            // Use DashMap's entry API for more efficient insertions
            cache.entry(request_id.clone()).or_insert(response.clone());
            state.quote_cache.entry(request_id.clone()).or_insert(response.clone());

            // Spawn a task to remove the cache entry after 10 minutes
            let cache_clone = Arc::clone(&cache);
            let quote_cache_clone = Arc::clone(&state.quote_cache);
            let req_id_clone = request_id.clone();
            tokio::spawn(async move {
                sleep(Duration::from_secs(600)).await;
                cache_clone.remove(&req_id_clone);
                quote_cache_clone.remove(&req_id_clone);
            });

            let result = json!({
                "requestId": request_id,
                "success": response["success"],
                "data": response.get("data").unwrap_or(&json!([]))
            });

            Ok(result)
        }
        Err(error) => {
            error!("Error in quote service: {}", error);
            Err(format!("Quote service failed: {}", error))
        }
    }
}

pub async fn process_quote_stream(params: Value, state: Arc<AppState>) -> mpsc::Receiver<Result<Value, String>> {
    println!("Processing quote stream with params: {:?}", params);

    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        let mut stream = route_quote_stream(params, Arc::clone(&state)).await;

        while let Some(result) = stream.recv().await {
            // Immediately forward each result as it's received
            if let Err(e) = tx.send(result).await {
                eprintln!("Error sending result through channel: {}", e);
                break;
            }
        }
    });

    rx
}

pub async fn process_transaction_from_quote(quote: &Value, state: &Arc<AppState>) -> Result<Value, String> {
    info!("Processing transaction from quote with data: {:?}", quote);

    if !is_valid_quote(quote) {
        return Err("Quote validation failed due to missing required fields".to_string());
    }

    route_transaction_from_quote(quote, state.clone()).await
}

fn is_valid_quote(quote: &Value) -> bool {
    let required_fields = [
        "tool", "fromChainId", "toChainId", "fromAmount", "fromAddress", "toAmount", 
        "fromToken", "toToken", "options", "toAddress"
    ];

    let missing_fields: Vec<&str> = required_fields.iter()
        .filter(|&&field| !quote.get(field).is_some())
        .copied()
        .collect();

    if !missing_fields.is_empty() {
        error!("Missing required fields in quote: {}", missing_fields.join(", "));
        return false;
    }

    true
}
