use serde_json::{Value, json};
use std::sync::Arc;
use crate::load_resources::AppState;
use crate::utils::utils::{get_random_proxy_client, get_replaced_addresses};
use crate::utils::format_swap_details::format_swap_details;
use futures::future::BoxFuture;
use futures::FutureExt;
use tracing::{error, debug};
use reqwest::Url;

// Global error logging function to ensure all errors are logged
fn log_error(context: &str, error_msg: &str) -> String {
    let full_error_msg = format!("{}: {}", context, error_msg);
    error!("{}", full_error_msg);  // Log the error with context
    full_error_msg
}

pub fn get_swap_quote(params: Value, state: Arc<AppState>) -> BoxFuture<'static, Result<Value, String>> {
    async move {
        debug!("Swap quote parameters: {:?}", params);

        // Extract necessary parameters with error handling
        let from_chain_id = params["fromChainId"].as_u64().ok_or("Invalid or missing fromChainId")?;
        let to_chain_id = params["toChainId"].as_u64().unwrap_or(from_chain_id);
        let amount = params["amount"].as_str().ok_or("Invalid or missing amount")?;
        let from_token_address = params["fromTokenAddress"].as_str().ok_or("Invalid or missing fromTokenAddress")?;
        let to_token_address = params["toTokenAddress"].as_str().ok_or("Invalid or missing toTokenAddress")?;
        let from_address = params["fromAddress"].as_str().ok_or("Invalid or missing fromAddress")?;
        let options = params["options"].as_object().ok_or("Invalid or missing options")?;
        let gas_prices = params["gasPrices"].as_array().ok_or("Invalid or missing gasPrices")?;
        let quote_only = params["quoteOnly"].as_bool().unwrap_or(false);
        let slippage_tolerance = options["slippage"].as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| options["slippage"].as_f64()).ok_or_else(|| "Invalid slippage".to_string())?;


        // Replace token addresses
        let (valid_from_token, valid_to_token) = get_replaced_addresses(
            from_token_address,
            to_token_address,
            from_chain_id,
            to_chain_id,
            "bungee",
            &state
        ).map_err(|e| {
            error!("Failed to replace token addresses: {}", e);
            e.to_string()
        })?;

        // Fetch the quote
        let quote = get_quote(
            valid_from_token.to_string(),
            valid_to_token.to_string(),
            amount,
            from_chain_id,
            to_chain_id,
            slippage_tolerance,
            from_address,
            from_address,  // Use from_address as to_address
            &state
        ).await.map_err(|e| log_error("Failed to fetch quote", &e))?;

        debug!("Fetched quote: {:?}", quote);

        // Build the transaction
        let transaction = build_transaction(quote.clone(), &state).await.map_err(|e| log_error("Failed to build transaction", &e))?;

        debug!("Built transaction: {:?}", transaction);

        // Extract necessary fields
        let to_amount = quote["userTxs"][0]["toAmount"]
            .as_str()
            .ok_or_else(|| log_error("Missing toAmount in quote", "toAmount is missing or not a valid string"))?;
        
        if to_amount.is_empty() {
            return Err(log_error("toAmount Error", "toAmount is missing or empty in the quote response"));
        }

        // Extract transaction details
        let tx_data = transaction["txData"].as_str().ok_or("Missing txData in transaction")?;
        let tx_target = transaction["txTarget"].as_str().ok_or("Missing txTarget in transaction")?;
        let value_hex = transaction["value"].as_str().ok_or("Missing value in transaction")?;
        
        // Convert value from hex to wei string
        let value_wei = u128::from_str_radix(value_hex.trim_start_matches("0x"), 16)
            .map(|v| v.to_string())
            .map_err(|e| format!("Failed to convert value to wei: {}", e))?;

        let approval_address = transaction["approvalData"]
            .as_str()
            .unwrap_or("0x0000000000000000000000000000000000000000");
        
        // Prepare quote data
        let quote_data = json!({
            "from": from_address,
            "to": tx_target,
            "chainID": from_chain_id,
            "data": tx_data,
            "value": value_wei,
        });

        // Extract gas_limit as a string only if quote_only is true
        let gas_limit: Option<String> = if quote_only {
            quote["userTxs"][0]["gasFees"]["gasLimit"]
                .as_u64()
                .or_else(|| quote["userTxs"][0]["gasFees"]["gasLimit"].as_f64().map(|v| v as u64))
                .map(|v| v.to_string())
        } else {
            None
        };

        debug!(
            "Preparing to call format_swap_details with the following parameters:\n\
            tool: 'bungee',\n\
            params: {:?},\n\
            quote_data: {:?},\n\
            toAmount: {:?},\n\
            approval_address: {:?},\n\
            gas_limit: {:?}",
            params,
            quote_data,
            to_amount,
            approval_address,
            gas_limit
        );

        // Call format_swap_details with appropriate arguments based on quote_only
        let formatted_data = if quote_only {
            format_swap_details(
                "bungee", 
                &params, 
                &quote_data, 
                &Value::String(to_amount.to_string()), 
                &Value::String(approval_address.to_string()), 
                &Value::Array(gas_prices.to_vec()),
                gas_limit.as_deref(), 
                None, 
                None, 
                &state
            ).await
        } else {
            format_swap_details(
                "bungee", 
                &params, 
                &quote_data, 
                &Value::String(to_amount.to_string()), 
                &Value::String(approval_address.to_string()), 
                &Value::Array(gas_prices.to_vec()),
                None,  // Pass null for gasLimit when quoteOnly is false
                None, 
                None, 
                &state
            ).await
        }.map_err(|e| log_error("Error formatting swap details", &e))?;

        debug!("Formatted swap details: {:?}", formatted_data);

        Ok(formatted_data)
    }.boxed()
}

async fn get_quote(
    from_token_address: String,
    to_token_address: String,
    amount: &str,
    from_chain_id: u64,
    to_chain_id: u64,
    slippage_tolerance: f64,
    from_address: &str,
    to_address: &str,
    state: &Arc<AppState>
) -> Result<Value, String> {
    let fee = state.settings["bungee"]["fee"].as_str().unwrap_or("");
    let api_key = state.settings["bungee"]["apiKey"].as_str().unwrap_or("");
    let disable_fee = state.settings["bungee"]["disableFee"].as_bool().unwrap_or(false);
    let referrer = state.settings["bungee"]["referrer"].as_str().unwrap_or("");

    let client = get_random_proxy_client(&state.proxy_clients)
        .ok_or("No proxy client available")?;

    // Use a vector instead of an array to dynamically push values.
    let mut params: Vec<(&str, String)> = vec![
        ("fromTokenAddress", from_token_address),
        ("toTokenAddress", to_token_address),
        ("fromAmount", amount.to_string()),
        ("fromChainId", from_chain_id.to_string()),
        ("toChainId", to_chain_id.to_string()),
        ("userAddress", from_address.to_string()),
        ("recipient", to_address.to_string()),
        ("uniqueRoutesPerBridge", true.to_string()),
        ("defaultBridgeSlippage", slippage_tolerance.to_string()),
        ("defaultSwapSlippage", slippage_tolerance.to_string()),
    ];

    if !disable_fee {
        params.push(("feeTakerAddress", referrer.to_string()));
        params.push(("feePercent", fee.to_string()));
    }

    let url = Url::parse("https://api.socket.tech/v2/quote").unwrap();
    let response = client.get(url)
        .query(&params)
        .header("API-KEY", api_key)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch quote: {}", e);
            format!("Failed to fetch quote: {}", e)
        })?;

    let result = response.json::<Value>().await
        .map_err(|e| {
            error!("Failed to parse quote response: {}", e);
            format!("Failed to parse quote response: {}", e)
        })?;

    debug!("Quote response: {:?}", result);  // Log quote response

    let routes = result["result"]["routes"].get(0)
        .ok_or("No valid routes found in quote")?;

    Ok(routes.clone())
}

async fn build_transaction(route: Value, state: &Arc<AppState>) -> Result<Value, String> {
    let api_key = state.settings["bungee"]["apiKey"].as_str().unwrap_or("");

    let client = get_random_proxy_client(&state.proxy_clients)
        .ok_or("No proxy client available")?;

    let body = json!({
        "route": route
    });

    let url = Url::parse("https://api.socket.tech/v2/build-tx").unwrap();
    let response = client.post(url)
        .json(&body)
        .header("API-KEY", api_key)
        .send()
        .await
        .map_err(|e| {
            error!("Failed to build transaction: {}", e);
            format!("Failed to build transaction: {}", e)
        })?;

    let transaction_data = response.json::<Value>().await
        .map_err(|e| {
            error!("Failed to parse transaction response: {}", e);
            format!("Failed to parse transaction response: {}", e)
        })?;

    debug!("Transaction response: {:?}", transaction_data);  // Log transaction response

    if !transaction_data["success"].as_bool().unwrap_or(false) {
        return Err("Error building transaction".to_string());
    }

    Ok(transaction_data["result"].clone())
}
