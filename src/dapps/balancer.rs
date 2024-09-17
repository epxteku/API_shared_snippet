use serde_json::{json, Value};
use std::sync::Arc;
use crate::load_resources::AppState;
use crate::utils::utils::{get_random_proxy_client, get_replaced_addresses};
use crate::utils::format_swap_details::format_swap_details;
use futures::future::BoxFuture;
use futures::FutureExt;
use tracing::{debug, error};

pub fn get_swap_quote(params: Value, state: Arc<AppState>) -> BoxFuture<'static, Result<Value, String>> {
    async move {

        let from_chain_id = params["fromChainId"].as_u64().ok_or("Invalid fromChainId")?;
        let to_chain_id = params["toChainId"].as_u64().unwrap_or(from_chain_id);
        let amount = params["amount"].as_str().ok_or("Invalid amount")?;
        let from_token_address = params["fromTokenAddress"].as_str().ok_or("Invalid fromTokenAddress")?;
        let to_token_address = params["toTokenAddress"].as_str().ok_or("Invalid toTokenAddress")?;
        let to_address = params["toAddress"].as_str().ok_or("Invalid toAddress")?;
        let from_address = params["fromAddress"].as_str().ok_or("Invalid fromAddress")?;
        let options = params["options"].as_object().ok_or("Invalid options")?;
        let gas_prices = params["gasPrices"].as_array().ok_or("Invalid gasPrices")?;
        let gas_price_wei = gas_prices.get(0)
            .and_then(|v| v.as_str())
            .ok_or("Invalid gasPriceGwei")?;
        let quote_only = params["quoteOnly"].as_bool().unwrap_or(false);
        
        let slippage_percentage = options["slippage"].as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| options["slippage"].as_f64()).ok_or_else(|| "Invalid slippage".to_string())?;


        // Replace token addresses
        let (valid_from_token, valid_to_token) = get_replaced_addresses(
            from_token_address,
            to_token_address,
            from_chain_id,
            to_chain_id,
            "balancer",
            &state
        ).map_err(|e| {
            error!("Failed to replace token addresses: {}", e);
            e.to_string()
        })?;

        debug!("Replaced addresses: from={}, to={}", valid_from_token, valid_to_token);

        // Fetch the quote
        let quote_result = get_quote(
            from_chain_id,
            &valid_from_token,
            &valid_to_token,
            amount,
            from_address,
            to_address,
            slippage_percentage,
            gas_price_wei,
            &state
        ).await.map_err(|e| {
            error!("Failed to retrieve quote: {}", e);
            e.to_string()
        })?;

        if quote_result.get("error").is_some() {
            error!("Balancer API error: {:?}", quote_result);
            return Err("Failed to retrieve a valid quote from Balancer.".to_string());
        }

        let zero_address = "0x0000000000000000000000000000000000000000";
        let value = if valid_from_token == zero_address {
            amount.to_string()
        } else {
            "0".to_string()
        };

        // Extract and convert buyAmount (BigNumber to string)
        let buy_amount_hex = quote_result["price"]["buyAmount"]["hex"]
            .as_str()
            .ok_or("Invalid buyAmount hex")?;
        let buy_amount = u128::from_str_radix(&buy_amount_hex[2..], 16)
            .map_err(|_| "Failed to parse buyAmount from hex")?;

        // Convert buyAmount to string
        let buy_amount_str = buy_amount.to_string();

        let transaction_data = json!({
            "from": from_address,
            "to": quote_result["to"],
            "chainID": from_chain_id,
            "data": quote_result["data"],
            "value": value,
        });

        debug!("Transaction data: {:?}", transaction_data);

        // Debugging all the parameters before calling format_swap_details
        debug!("Calling format_swap_details with the following parameters:");
        debug!("tool: 'balancer'");
        debug!("params: {:?}", params);
        debug!("transaction_data: {:?}", transaction_data);
        debug!("buyAmount: {:?}", buy_amount_str); // Passing extracted buyAmount
        debug!("allowanceTarget: {:?}", quote_result["price"]["allowanceTarget"]);
        debug!("gasPrices: {:?}", params["gasPrices"]);
        debug!("gasEstimate: None");
        debug!("additional_fee: None");
        debug!("dapp_options: None");

        let formatted_data = if quote_only {
            format_swap_details(
                "balancer",
                &params,
                &transaction_data,
                &json!(buy_amount_str), // Pass buyAmount
                &quote_result["price"]["allowanceTarget"],
                &params["gasPrices"],
                Some("0"), 
                None,
                None,
                &state
            ).await.map_err(|e| {
                error!("Error formatting quote-only swap details: {}", e);
                e.to_string()
            })?
        } else {
            format_swap_details(
                "balancer",
                &params,
                &transaction_data,
                &json!(buy_amount_str), // Pass buyAmount
                &quote_result["price"]["allowanceTarget"],
                &params["gasPrices"],
                None,
                None,
                None,
                &state
            ).await.map_err(|e| {
                error!("Error formatting swap details: {}", e);
                e.to_string()
            })?
        };

        debug!("Formatted swap details: {:?}", formatted_data);
        Ok(formatted_data)
    }.boxed()
}


async fn get_quote(
    from_chain_id: u64,
    from_token_address: &String,
    to_token_address: &String,
    amount: &str,
    from_address: &str,
    to_address: &str,
    slippage_percentage: f64,
    gas_price_wei: &str,
    state: &Arc<AppState>,
) -> Result<Value, String> {
    let client = get_random_proxy_client(&state.proxy_clients)
        .ok_or("No proxy client available")?;

    let params = json!({
        "sellToken": from_token_address,
        "buyToken": to_token_address,
        "orderKind": "sell",
        "amount": amount,
        "gasPrice": gas_price_wei,
        "sender": from_address,
        "receiver": to_address,
        "slippagePercentage": slippage_percentage / 100.0,
    });

    debug!("Sending request to Balancer API with params: {:?}", params);

    let response = client.post(&format!("https://api.balancer.fi/order/{}", from_chain_id))
        .json(&params)
        .send()
        .await
        .map_err(|e| {
            error!("Error sending request to Balancer API: {}", e);
            format!("Error fetching quote: {}", e)
        })?;

    let result = response.json::<Value>().await.map_err(|e| {
        error!("Error parsing Balancer API response: {}", e);
        format!("Error parsing quote response: {}", e)
    })?;

    debug!("Received Balancer API response: {:?}", result);

    Ok(result)
}
