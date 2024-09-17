use serde_json::Value;
use std::sync::Arc;
use std::fs;
use lazy_static::lazy_static;
use crate::load_resources::AppState;
use crate::utils::utils::get_random_proxy_client;
use crate::utils::format_swap_details::format_swap_details;
use std::path::PathBuf;
use ethers::types::U256;
use futures::future::BoxFuture;
use futures::FutureExt;

lazy_static! {
    static ref PATHS: Value = {
        let mut paths_file_path = PathBuf::from(file!());
        paths_file_path.pop(); // Remove "jumper.rs"
        paths_file_path.push("abi/jumper/paths.json");

        let paths_content = fs::read_to_string(&paths_file_path)
            .expect(&format!("Failed to read paths.json from {:?}", paths_file_path));

        serde_json::from_str(&paths_content)
            .expect("Failed to parse paths.json")
    };
    static ref CHAINS: Vec<Value> = PATHS["chains"].as_array()
        .expect("Invalid chains in paths.json")
        .clone();
}

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
        let quote_only = params["quoteOnly"].as_bool().unwrap_or(false);
        let slippage_percentage = options["slippage"].as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| options["slippage"].as_f64()).map(|v| v / 100.0).ok_or_else(|| "Invalid slippage".to_string())?;
        let from_chain_exists = CHAINS.iter().any(|chain| chain["id"].as_u64() == Some(from_chain_id));
        let to_chain_exists = CHAINS.iter().any(|chain| chain["id"].as_u64() == Some(to_chain_id));

        if !from_chain_exists || !to_chain_exists {
            return Err("One or both chain IDs are not supported.".to_string());
        }

        let quote = get_quote(
            from_chain_id,
            to_chain_id,
            from_token_address,
            to_token_address,
            amount,
            from_address,
            to_address,
            slippage_percentage,
            &state,
        ).await?;

        if let Some(mut quote) = quote {
            if let Some(transaction_request) = quote["transactionRequest"].as_object_mut() {
                if let Some(value) = transaction_request.get("value") {
                    if let Some(hex_value) = value.as_str() {
                        if let Ok(wei_value) = U256::from_str_radix(hex_value.trim_start_matches("0x"), 16) {
                            transaction_request["value"] = Value::String(wei_value.to_string());
                        }
                    }
                }

                if let Some(gas_price) = transaction_request.get("gasPrice") {
                    if let Some(hex_gas_price) = gas_price.as_str() {
                        if let Ok(wei_gas_price) = U256::from_str_radix(hex_gas_price.trim_start_matches("0x"), 16) {
                            transaction_request["gasPrice"] = Value::String(wei_gas_price.to_string());
                        }
                    }
                }
            }

            let formatted_data = if quote_only {
                format_swap_details(
                    "jumper",
                    &params,
                    &quote["transactionRequest"],
                    &quote["estimate"]["toAmount"],
                    &quote["estimate"]["approvalAddress"],
                    &params["gasPrices"],
                    Some(quote["estimate"]["gasCosts"][0]["limit"].as_str().unwrap_or("0")),
                    None, // No additional fee
                    None, // No dapp_options
                    &state
                ).await?
            } else {
                format_swap_details(
                    "jumper",
                    &params,
                    &quote["transactionRequest"],
                    &quote["estimate"]["toAmount"],
                    &quote["estimate"]["approvalAddress"],
                    &params["gasPrices"],
                    None, // No gas estimate
                    None, // No additional fee
                    None, // No dapp_options
                    &state
                ).await?
            };

            Ok(formatted_data)
        } else {
            Err("Unable to process quote.".to_string())
        }
    }.boxed()
}

async fn get_quote(
    from_chain: u64,
    to_chain: u64,
    from_token: &str,
    to_token: &str,
    from_amount: &str,
    from_address: &str,
    to_address: &str,
    slippage_percentage: f64,
    state: &Arc<AppState>,
) -> Result<Option<Value>, String> {
    let jumper_settings = state.settings["jumper"].as_object()
        .ok_or("Jumper settings not found")?;
    let referrer = jumper_settings.get("referrer")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let fee = jumper_settings.get("fee")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let client = get_random_proxy_client(&state.proxy_clients)
        .ok_or("No proxy client available")?;

    let params = [
        ("fromChain", from_chain.to_string()),
        ("toChain", to_chain.to_string()),
        ("fromToken", from_token.to_string()),
        ("toToken", to_token.to_string()),
        ("fromAmount", from_amount.to_string()),
        ("fromAddress", from_address.to_string()),
        ("toAddress", to_address.to_string()),
        ("slippage", slippage_percentage.to_string()),
        ("fee", fee.to_string()),
        ("referrer", referrer.to_string()),
    ];

    let response = client.get("https://li.quest/v1/quote")
        .query(&params)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch quote: {}", e))?;

    let result = response.json::<Value>()
        .await
        .map_err(|e| format!("Failed to parse quote response: {}", e))?;

    Ok(Some(result))
}
