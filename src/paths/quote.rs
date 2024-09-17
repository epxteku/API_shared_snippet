//src/paths/quote.rs
use axum::{Json, Router, routing::{post, get}, extract::Query, extract::Extension, http::StatusCode};
use std::sync::Arc;
use std::collections::HashMap;
use crate::services::quote_service::process_quote;
use crate::load_resources::AppState;
use crate::paths::validate_params::{validate_required_params, format_options};
use serde::{Deserialize, Serialize, Deserializer};
use serde_json::Value;
use tracing::error;


#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuoteParams {
    #[serde(deserialize_with = "crate::paths::utils::deserialization_helpers::string_or_number_to_u32")]
    pub from_chain_id: u32,
    pub from_address: String,
    #[serde(deserialize_with = "crate::paths::utils::deserialization_helpers::number_to_string")]
    pub amount: String,
    pub from_token_address: String,
    pub to_token_address: String,
    pub to_address: Option<String>,
    #[serde(deserialize_with = "crate::paths::utils::deserialization_helpers::string_or_number_to_option_u32", default)]
    pub to_chain_id: Option<u32>,
    #[serde(default)]
    pub dapps: Vec<String>,  // Axum will handle repeated query parameters as a Vec<String>
    pub options: Option<Value>,
    #[serde(flatten)]
    pub other_params: HashMap<String, Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetQuoteParams {
    #[serde(deserialize_with = "crate::paths::utils::deserialization_helpers::string_or_number_to_u32")]
    pub from_chain_id: u32,
    pub from_address: String,
    #[serde(deserialize_with = "crate::paths::utils::deserialization_helpers::number_to_string")]
    pub amount: String,
    pub from_token_address: String,
    pub to_token_address: String,
    pub to_address: Option<String>,
    #[serde(deserialize_with = "crate::paths::utils::deserialization_helpers::string_or_number_to_option_u32", default)]
    pub to_chain_id: Option<u32>,
    #[serde(deserialize_with = "crate::paths::utils::deserialization_helpers::string_or_seq")]  // Custom deserializer for dapps
    pub dapps: Vec<String>,
    #[serde(deserialize_with = "crate::paths::utils::deserialization_helpers::string_or_number_to_f64", default)]  // Custom deserializer for slippage
    pub slippage: f64,  // Ensure slippage is present in the struct
    pub options: Option<serde_json::Value>,
    #[serde(flatten)]
    pub other_params: std::collections::HashMap<String, Vec<String>>,
}


// Custom deserializer for comma-separated dapps
pub fn comma_separated<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    Ok(s.split(',')
        .map(|v| v.trim().to_string())
        .collect())
}

async fn handle_quote_request(
    Extension(state): Extension<Arc<AppState>>, 
    params: QuoteParams,
    is_post: bool,
) -> Result<Json<Value>, StatusCode> {
    tracing::info!("Handling request for /api/quote");

    let validation = validate_required_params(&params, &state);
    if !validation.valid {
        error!("Validation failed: {}", validation.message);
        return Ok(Json(serde_json::json!({
            "success": false,
            "message": validation.message
        })));
    }

    let to_address = params.to_address.clone().unwrap_or(params.from_address.clone());
    let to_chain_id = params.to_chain_id.unwrap_or(params.from_chain_id);

    let options = if is_post {
        params.options.unwrap_or(serde_json::json!({}))
    } else {
        format_options(&params.other_params)
    };

    let transaction_params = serde_json::json!({
        "fromChainId": params.from_chain_id,
        "fromAddress": params.from_address,
        "amount": params.amount,
        "fromTokenAddress": params.from_token_address,
        "toTokenAddress": params.to_token_address,
        "toAddress": to_address,
        "toChainId": to_chain_id,
        "options": options
    });

    // Print the formatted quote JSON
    tracing::info!("Formatted quote JSON: {}", serde_json::to_string_pretty(&transaction_params).unwrap());

    tracing::debug!("Transaction params: {:?}", transaction_params);

    match process_quote(transaction_params, Arc::clone(&state.tokens), Arc::clone(&state)).await {
        Ok(response) => {
            tracing::debug!("Quote processed successfully: {:?}", response);
            Ok(Json(response))
        }
        Err(error) => {
            tracing::error!("Error processing quote: {}", error);
            Ok(Json(serde_json::json!({
                "success": false,
                "message": format!("Server error: {}", error)
            })))
        }
    }
}

pub async fn post_quote_handler(
    Extension(state): Extension<Arc<AppState>>, 
    Json(params): Json<QuoteParams>,
) -> Result<Json<Value>, StatusCode> {
    tracing::info!("Received POST /api/quote request");
    handle_quote_request(Extension(state), params, true).await
}

pub async fn get_quote_handler(
    Extension(state): Extension<Arc<AppState>>, 
    Query(params): Query<GetQuoteParams>,  // Use GetQuoteParams for GET requests
) -> Result<Json<Value>, StatusCode> {
    tracing::info!("Received GET /api/quote request");

    // Convert GetQuoteParams to QuoteParams before passing to handle_quote_request
    let params = QuoteParams {
        from_chain_id: params.from_chain_id,
        from_address: params.from_address,
        amount: params.amount,
        from_token_address: params.from_token_address,
        to_token_address: params.to_token_address,
        to_address: params.to_address,
        to_chain_id: params.to_chain_id,
        dapps: params.dapps.clone(),  // Pass dapps directly
        options: Some(serde_json::json!({
            "slippage": params.slippage,  // Pass slippage under options
            "dapps": params.dapps,  // Also pass dapps under options
        })),
        other_params: params.other_params,
    };

    handle_quote_request(Extension(state), params, false).await
}


pub fn create_quote_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/quote", post(post_quote_handler))
        .route("/api/quote", get(get_quote_handler))
        .layer(Extension(state))
}