//src/services/quote_router.rs
use crate::utils::filter_dapps::filter_dapps;
use crate::dapps::AVAILABLE_SERVICES;
use crate::utils::utils::fetch_gas_price;
use crate::load_resources::AppState;
use crate::utils::fetch_token_details::fetch_token_details;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::timeout;
use std::sync::Arc;
use tracing::{debug, error};
use futures::future::join_all;

pub async fn route_quote(params: Value, state: Arc<AppState>) -> Result<Value, String> {
    // Clone the params and state to avoid lifetime issues in tasks
    let mut extended_params = params.clone();
    let from_chain_id = params["fromChainId"].as_u64().ok_or("Invalid fromChainId")?;
    let to_chain_id = params["toChainId"].as_u64().unwrap_or(from_chain_id);
    let from_token_address = params["fromTokenAddress"].as_str().ok_or("Invalid fromTokenAddress")?;
    let to_token_address = params["toTokenAddress"].as_str().ok_or("Invalid toTokenAddress")?;
    
    // Define the native zero address
    let zero_address = "0x0000000000000000000000000000000000000000";

    let gas_prices = fetch_gas_price(from_chain_id, Arc::clone(&state)).await
        .map_err(|e| format!("Failed to fetch gas prices: {}", e))?;

    // Extend params with gas prices and set quoteOnly
    extended_params["gasPrices"] = json!(gas_prices);
    extended_params["quoteOnly"] = json!(true);

    // Filter available dapps
    let available_dapps_names = filter_dapps(
        from_token_address,
        from_chain_id,
        to_chain_id,
        Arc::clone(&state)
    );

    debug!("Available DApps: {:?}", available_dapps_names);

    if available_dapps_names.is_empty() {
        return Ok(json!({
            "success": false,
            "message": "No dApps available"
        }));
    }

    // Create a longer-lived Arc clone
    let state_clone = Arc::clone(&state);

    // Pass all tokens (fromToken, toToken, and the zero address token) to fetch_token_details at once
    let tokens_with_chain_ids = vec![
        (from_token_address, from_chain_id),
        (to_token_address, to_chain_id),
        (zero_address, from_chain_id), // Native token (zero address)
    ];

    let token_details = fetch_token_details(tokens_with_chain_ids, &state_clone).await
        .map_err(|e| format!("Failed to fetch token details: {}", e))?;

    // Assume token_details contains details for all tokens in the order they were passed
    let from_token_details = &token_details[0];
    let to_token_details = &token_details[1];
    let native_token_details = &token_details[2];

    extended_params["fromTokenDetails"] = json!(from_token_details);
    extended_params["toTokenDetails"] = json!(to_token_details);
    extended_params["nativeTokenDetails"] = json!(native_token_details); 

    let services_to_run: Vec<_> = if let Some(options) = params.get("options") {
        if let Some(dapps) = options.get("dapps").and_then(|d| d.as_array()) {
            if dapps.is_empty() {
                // If dapps array is empty, use all available dapps
                available_dapps_names.iter()
                    .filter(|dapp| AVAILABLE_SERVICES.contains_key(dapp.as_str()))
                    .map(|dapp| (dapp.to_string(), AVAILABLE_SERVICES[dapp.as_str()]))
                    .collect()
            } else {
                // If dapps array is not empty, use only the specified dapps
                dapps.iter()
                    .filter_map(|dapp| dapp.as_str())
                    .filter(|&dapp| available_dapps_names.contains(&dapp.to_string()) && AVAILABLE_SERVICES.contains_key(dapp))
                    .map(|dapp| (dapp.to_string(), AVAILABLE_SERVICES[dapp]))
                    .collect()
            }
        } else {
            // If dapps key is not present, use all available dapps
            available_dapps_names.iter()
                .filter(|dapp| AVAILABLE_SERVICES.contains_key(dapp.as_str()))
                .map(|dapp| (dapp.to_string(), AVAILABLE_SERVICES[dapp.as_str()]))
                .collect()
        }
    } else {
        // If options is not present, use all available dapps
        available_dapps_names.iter()
            .filter(|dapp| AVAILABLE_SERVICES.contains_key(dapp.as_str()))
            .map(|dapp| (dapp.to_string(), AVAILABLE_SERVICES[dapp.as_str()]))
            .collect()
    };

    if services_to_run.is_empty() {
        return Ok(json!({
            "success": false,
            "message": "No valid quotes found."
        }));
    }

    // Prepare the list of futures without spawning tasks
    let futures = services_to_run.into_iter().map(|(name, service)| {
        let params_clone = extended_params.clone();
        let state_clone = Arc::clone(&state);
        async move {
            match timeout(Duration::from_secs(30), service(params_clone, state_clone)).await {
                Ok(data) => match data {
                    Ok(value) => {
                        if validate_response_format(&value) {
                            Some(json!({
                                "name": name,
                                "data": value
                            }))
                        } else {
                            error!("Invalid response format from {}: {:?}", name, value);
                            None
                        }
                    },
                    Err(e) => {
                        error!("Error in {}: {}", name, e);
                        None
                    }
                },
                Err(_) => {
                    error!("Timeout for {}", name);
                    None
                }
            }
        }
    });

    // Execute all futures concurrently
    let results: Vec<_> = join_all(futures).await
        .into_iter()
        .filter_map(|res| res)
        .collect();
    

    if results.is_empty() {
        return Ok(json!({
            "success": false,
            "message": "No valid quotes found."
        }));
    }

    // Sort and structure the response
    let mut sorted_results: Vec<Value> = results.into_iter()
        .enumerate()
        .map(|(index, result)| {
            json!({
                "id": index + 1,
                "name": result["name"],
                "data": result["data"]
            })
        })
        .collect();

    sorted_results.sort_by(|a, b| {
        let a_amount = a["data"]["toAmount"].as_str().unwrap_or("0");
        let b_amount = b["data"]["toAmount"].as_str().unwrap_or("0");
        b_amount.cmp(a_amount)
    });

    Ok(json!({
        "success": true,
        "data": sorted_results
    }))
}

fn validate_response_format(data: &Value) -> bool {
    data.get("tool").is_some() &&
    data.get("fromChainId").is_some() &&
    data.get("fromAmount").is_some() &&
    data.get("toAmount").is_some() &&
    data.get("fromToken").is_some() &&
    data.get("toToken").is_some() &&
    data.get("transaction").is_some()
}
