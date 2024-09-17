use serde_json::{Value, json};
use std::sync::Arc;
use crate::load_resources::AppState;
use crate::utils::utils::{get_random_proxy_client, get_replaced_addresses};
use crate::utils::format_swap_details::format_swap_details;
use futures::future::BoxFuture;
use futures::FutureExt;
use tracing::{error, debug};
use reqwest::Url;

pub fn get_swap_quote(params: Value, state: Arc<AppState>) -> BoxFuture<'static, Result<Value, String>> {
    async move {
        debug!("Swap quote parameters: {:?}", params);

        // Extract necessary parameters with error handling
        let from_chain_id = params["fromChainId"].as_u64().ok_or("Invalid or missing fromChainId")?;
        let to_chain_id = params["toChainId"].as_u64().unwrap_or(from_chain_id);
        let amount = params["amount"].as_str().ok_or("Invalid or missing amount")?;
        let from_token_address = params["fromTokenAddress"].as_str().ok_or("Invalid or missing fromTokenAddress")?;
        let to_token_address = params["toTokenAddress"].as_str().ok_or("Invalid or missing toTokenAddress")?;
        let to_address = params["toAddress"].as_str().ok_or("Invalid or missing toAddress")?;
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
            "debridge",
            &state
        ).map_err(|e| {
            error!("Failed to replace token addresses: {}", e);
            e.to_string()
        })?;

        // Fetch the quote
        let quote = get_quote(
            from_chain_id,
            &valid_from_token,
            amount,
            to_chain_id,
            &valid_to_token,
            from_address,
            to_address,
            slippage_tolerance,
            &state
        ).await.map_err(|e| format!("Failed to fetch a valid quote: {}", e))?;

        // Prepare quoteData from the quote response
        let quote_data = json!({
            "from": from_address,
            "to": quote["tx"]["to"],
            "chainID": from_chain_id,
            "data": quote["tx"]["data"],
            "value": quote["tx"]["value"],
        });

        let approval_target = quote["tx"]["to"]
            .as_str()
            .unwrap_or("0x0000000000000000000000000000000000000000");

        // Check for receive value, stop if not found
        let receive_value = match quote["estimation"]["dstChainTokenOut"]["amount"].as_str() {
            Some(value) => value,
            None => {
                error!("Missing receive value in quote");
                return Ok(json!({
                    "success": false,
                    "message": "Missing receive value in quote"
                }));
            }
        };

        let fix_fee = quote["fixFee"].as_str().unwrap_or("0");

        // Convert fix_fee to a Value
        let fix_fee_value = Value::String(fix_fee.to_string());

        debug!(
            "Preparing to call format_swap_details with the following parameters:\n\
            tool: 'debridge',\n\
            params: {:?},\n\
            quote_data: {:?},\n\
            receive_value: {:?},\n\
            approval_target: {:?},\n\
            fix_fee: {:?}",
            params,
            quote_data,
            receive_value,
            approval_target,
            fix_fee_value
        );

        // Call format_swap_details with appropriate arguments based on quote_only
        let formatted_data = if quote_only {
            format_swap_details(
                "debridge", 
                &params, 
                &quote_data, 
                &Value::String(receive_value.to_string()), 
                &Value::String(approval_target.to_string()), 
                &Value::Array(gas_prices.to_vec()),
                Some("quote"), 
                Some(&fix_fee_value), 
                None, 
                &state
            ).await
        } else {
            format_swap_details(
                "debridge", 
                &params, 
                &quote_data, 
                &Value::String(receive_value.to_string()), 
                &Value::String(approval_target.to_string()), 
                &Value::Array(gas_prices.to_vec()),
                None, 
                Some(&fix_fee_value), 
                None, 
                &state
            ).await
        }.map_err(|e| format!("Error formatting swap details: {}", e))?;

        debug!("Formatted swap details: {:?}", formatted_data);

        Ok(formatted_data)
    }.boxed()
}

async fn get_quote(
    from_chain_id: u64,
    valid_from_token: &str,
    amount: &str,
    to_chain_id: u64,
    valid_to_token: &str,
    from_address: &str,
    to_address: &str,
    slippage: f64,
    state: &Arc<AppState>
) -> Result<Value, String> {
    let client = get_random_proxy_client(&state.proxy_clients)
        .ok_or("No proxy client available")?;

    let url = Url::parse("https://dln.debridge.finance/v1.0/dln/order/create-tx").unwrap();

    let mut params = vec![
        ("srcChainId", from_chain_id.to_string()),
        ("srcChainTokenIn", valid_from_token.to_string()),
        ("srcChainTokenInAmount", amount.to_string()),
        ("dstChainId", to_chain_id.to_string()),
        ("dstChainTokenOut", valid_to_token.to_string()),
        ("dstChainTokenOutAmount", "auto".to_string()),
        ("senderAddress", from_address.to_string()),
        ("dstChainTokenOutRecipient", to_address.to_string()),
        ("srcChainOrderAuthorityAddress", from_address.to_string()),
        ("dstChainOrderAuthorityAddress", from_address.to_string()),
        ("prependOperatingExpense", "true".to_string()),
        ("slippage", slippage.to_string()),
        ("referralCode", state.settings["debridge"]["referralCode"].as_str().unwrap_or("").to_string()),
    ];

    if !state.settings["debridge"]["disableFee"].as_bool().unwrap_or(false) {
        params.push(("affiliateFeePercent", state.settings["debridge"]["fee"].as_str().unwrap_or("").to_string()));
        params.push(("affiliateFeeRecipient", state.settings["debridge"]["referrer"].as_str().unwrap_or("").to_string()));
    }

    let response = client.get(url)
        .query(&params)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    let result = response.json::<Value>().await
        .map_err(|e| format!("Failed to parse quote response: {}", e))?;

    Ok(result)
}