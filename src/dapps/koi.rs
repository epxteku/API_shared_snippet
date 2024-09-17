use ethers::prelude::*;
use ethers::utils::hex;
use serde_json::{json, Value};
use std::sync::Arc;
use std::str::FromStr;
use futures::future::BoxFuture;
use futures::FutureExt;
use tracing::debug;
use crate::utils::utils::{get_replaced_addresses, get_random_rpc_proxy_provider};
use crate::utils::format_swap_details::format_swap_details;
use crate::load_resources::AppState;

// Constants
const ROUTER_ADDRESS: &str = "0x8B791913eB07C32779a16750e3868aA8495F5964";

lazy_static::lazy_static! {
    static ref ROUTER_ABI: ethers::abi::Abi = {
        let abi_str = include_str!("./abi/koi/router.json");
        serde_json::from_str(abi_str).expect("Failed to parse ABI")
    };
}

pub fn get_swap_quote(params: Value, state: Arc<AppState>) -> BoxFuture<'static, Result<Value, String>> {
    async move {
        debug!("Starting get_swap_quote with params: {:?}", params);

        let from_chain_id = params["fromChainId"].as_u64().ok_or("Invalid fromChainId")?;
        let amount = params["amount"].as_str().ok_or("Invalid amount")?;
        let from_token_address = params["fromTokenAddress"].as_str().ok_or("Invalid fromTokenAddress")?;
        let to_token_address = params["toTokenAddress"].as_str().ok_or("Invalid toTokenAddress")?;
        let to_address = params["toAddress"].as_str().ok_or("Invalid toAddress")?;
        let from_address = params["fromAddress"].as_str().ok_or("Invalid fromAddress")?;
        let quote_only = params["quoteOnly"].as_bool().unwrap_or(false);
        let options = params["options"].as_object().ok_or("Invalid options")?;
        let slippage = options["slippage"].as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| options["slippage"].as_f64()).ok_or_else(|| "Invalid slippage".to_string())?;


        let zero_address = "0x0000000000000000000000000000000000000000";
        let eth_marker_address = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

        let is_from_eth = [zero_address, eth_marker_address].contains(&from_token_address.to_lowercase().as_str());
        let is_to_eth = [zero_address, eth_marker_address].contains(&to_token_address.to_lowercase().as_str());

        let provider = get_random_rpc_proxy_provider(from_chain_id, &state.rpc_proxy_providers)
            .ok_or_else(|| "No RPC provider available".to_string())?;
        let router_contract = Contract::new(Address::from_str(ROUTER_ADDRESS).unwrap(), ROUTER_ABI.clone(), provider.clone());

        let replaced_addresses = get_replaced_addresses(from_token_address, to_token_address, from_chain_id, from_chain_id, "koi", &state)
            .map_err(|e| e.to_string())?;
        let modified_from_token_address = replaced_addresses.0;
        let modified_to_token_address = replaced_addresses.1;


        let stable = false; // Adjust based on your specific requirements

        let (reserve_a, reserve_b): (U256, U256) = router_contract.method::<_, (U256, U256)>("getReserves", (
            Address::from_str(&modified_from_token_address).map_err(|e| e.to_string())?,
            Address::from_str(&modified_to_token_address).map_err(|e| e.to_string())?,
            stable
        )).map_err(|e| e.to_string())?
        .call()
        .await
        .map_err(|e| format!("Failed to get reserves: {}", e))?;

        let amount_out: U256 = router_contract.method::<_, U256>("quote", (
            U256::from_dec_str(amount).map_err(|e| e.to_string())?,
            reserve_a,
            reserve_b
        )).map_err(|e| e.to_string())?
        .call()
        .await
        .map_err(|e| format!("Failed to get quote: {}", e))?;

        if quote_only {
            let quote_data = json!({
                "from": from_address,
                "to": ROUTER_ADDRESS,
                "chainID": from_chain_id,
                "data": "quote",
                "value": "quote",
            });
            format_swap_details(
                "koi",
                &params,
                &quote_data,
                &Value::String(amount_out.to_string()),
                &Value::String(ROUTER_ADDRESS.to_string()),
                &params["gasPrices"],
                Some("quote"),
                None,
                None,
                &state
            ).await.map_err(|e| format!("Failed to format swap details: {}", e))
        } else {
            let slippage_adjusted = (100.0 - slippage) / 100.0;
            let amount_out_min = (amount_out.as_u128() as f64 * slippage_adjusted).floor() as u128;

            let deadline = U256::from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 1800);
            let path = vec![
                Address::from_str(&modified_from_token_address).map_err(|e| e.to_string())?,
                Address::from_str(&modified_to_token_address).map_err(|e| e.to_string())?,
            ];

            let (function_name, function_params): (&str, Vec<ethers::abi::Token>) = if is_from_eth {
                ("swapExactETHForTokens", vec![
                    ethers::abi::Token::Uint(U256::from(amount_out_min)),
                    ethers::abi::Token::Array(path.iter().map(|&p| ethers::abi::Token::Address(p)).collect()),
                    ethers::abi::Token::Address(Address::from_str(to_address).map_err(|e| e.to_string())?),
                    ethers::abi::Token::Uint(deadline),
                    ethers::abi::Token::Array(vec![ethers::abi::Token::Bool(stable)]),
                ])
            } else if is_to_eth {
                ("swapExactTokensForETH", vec![
                    ethers::abi::Token::Uint(U256::from_dec_str(amount).map_err(|e| e.to_string())?),
                    ethers::abi::Token::Uint(U256::from(amount_out_min)),
                    ethers::abi::Token::Array(path.iter().map(|&p| ethers::abi::Token::Address(p)).collect()),
                    ethers::abi::Token::Address(Address::from_str(to_address).map_err(|e| e.to_string())?),
                    ethers::abi::Token::Uint(deadline),
                    ethers::abi::Token::Array(vec![ethers::abi::Token::Bool(stable)]),
                ])
            } else {
                ("swapExactTokensForTokens", vec![
                    ethers::abi::Token::Uint(U256::from_dec_str(amount).map_err(|e| e.to_string())?),
                    ethers::abi::Token::Uint(U256::from(amount_out_min)),
                    ethers::abi::Token::Array(path.iter().map(|&p| ethers::abi::Token::Address(p)).collect()),
                    ethers::abi::Token::Address(Address::from_str(to_address).map_err(|e| e.to_string())?),
                    ethers::abi::Token::Uint(deadline),
                    ethers::abi::Token::Array(vec![ethers::abi::Token::Bool(stable)]),
                ])
            };


            let function = router_contract.abi().function(function_name)
                .map_err(|e| format!("Failed to get function {}: {:?}", function_name, e))?;

            let encoded = function.encode_input(&function_params)
                .map_err(|e| format!("Failed to encode function call: {}. Function: {}, Params: {:?}", e, function_name, function_params))?;

            let tx = json!({
                "from": from_address,
                "to": ROUTER_ADDRESS,
                "data": format!("0x{}", hex::encode(encoded)),
                "value": if is_from_eth { amount } else { "0" },
            });

            format_swap_details(
                "koi",
                &params,
                &tx,
                &Value::String(amount_out.to_string()),
                &Value::String(ROUTER_ADDRESS.to_string()),
                &params["gasPrices"],
                None,
                None,
                None,
                &state
            ).await.map_err(|e| format!("Failed to format swap details: {}", e))
        }
    }.boxed()
}