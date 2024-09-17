use ethers::prelude::*;
use ethers::types::{TransactionRequest, U256, Address, Bytes, U64, NameOrAddress};
use ethers::types::transaction::eip2718::TypedTransaction;
use serde_json::{json, Value};
use std::sync::Arc;
use std::str::FromStr;
use crate::load_resources::AppState;
use crate::utils::utils::get_random_rpc_proxy_provider;
use tracing::debug;


async fn estimate_gas_limit(chain_id: u64, transaction: &Value, state: &Arc<AppState>) -> Result<Option<U256>, String> {
    if transaction.as_object().unwrap().values().any(|v| v.as_str() == Some("quote")) {
        return Ok(None);
    }

    let provider = get_random_rpc_proxy_provider(chain_id, &state.rpc_proxy_providers)
        .ok_or("No provider available")?;

    // Convert the JSON transaction to a TransactionRequest
    let tx_request = TransactionRequest {
        from: Some(Address::from_str(transaction["from"].as_str().unwrap_or_default()).map_err(|e| format!("Invalid from address: {}", e))?),
        to: Some(NameOrAddress::Address(Address::from_str(transaction["to"].as_str().unwrap_or_default()).map_err(|e| format!("Invalid to address: {}", e))?)),
        value: Some(U256::from_dec_str(transaction["value"].as_str().unwrap_or("0")).map_err(|e| format!("Invalid value: {}", e))?),
        data: Some(Bytes::from_str(transaction["data"].as_str().unwrap_or("0x")).map_err(|e| format!("Invalid data: {}", e))?),
        nonce: None,
        gas: Some(U256::from_dec_str(transaction["gas"].as_str().unwrap_or("0")).map_err(|e| format!("Invalid gas: {}", e))?),
        gas_price: Some(U256::from_dec_str(transaction["gasPrice"].as_str().unwrap_or("0")).map_err(|e| format!("Invalid gas price: {}", e))?),
        chain_id: Some(U64::from(transaction["chainId"].as_u64().unwrap_or(chain_id))),
    };

    // Create a TypedTransaction::Legacy
    let typed_tx = TypedTransaction::Legacy(tx_request);

    match provider.estimate_gas(&typed_tx, None).await {
        Ok(gas) => Ok(Some(gas)),
        Err(e) => {
            eprintln!("Error estimating gas with proxy: {}", e);
            Ok(None)
        }
    }
}

pub async fn format_swap_details(
    tool: &str,
    params: &Value,
    transaction: &Value,
    to_amount: &Value,
    approval_address: &Value,
    gas_data: &Value,
    gas_estimate: Option<&str>,
    additional_fee: Option<&Value>,
    dapp_options: Option<&Value>,
    state: &Arc<AppState>
) -> Result<Value, String> {
    debug!("tool: {}, params: {:?}, transaction: {:?}, to_amount: {:?}, approval_address: {:?}, gas_data: {:?}, gas_estimate: {:?}, additional_fee: {:?}, dapp_options: {:?}",
           tool, params, transaction, to_amount, approval_address, gas_data, gas_estimate, additional_fee, dapp_options);

    let from_chain_id = params["fromChainId"].as_u64().ok_or("Invalid fromChainId")?;
    let amount = params["amount"].as_str().ok_or("Invalid amount")?;
    let to_address = params["toAddress"].as_str().ok_or("Invalid toAddress")?;
    let to_chain_id = params["toChainId"].as_u64().unwrap_or(from_chain_id);
    let options = params["options"].as_object().ok_or("Invalid options")?;
    let from_address = params["fromAddress"].as_str().ok_or("Invalid fromAddress")?;

    let mut tx = transaction.clone();

    // Handle gas estimation first, as it's the most time-consuming part
    let needs_gas_estimate = gas_estimate.is_none() && !transaction.as_object().unwrap().values().any(|v| v.as_str() == Some("quote"));
    let estimated_gas = if needs_gas_estimate {
        match estimate_gas_limit(from_chain_id, &tx, state).await {
            Ok(Some(gas)) => {
                let adjusted_gas: U256 = gas * 3 / 2;
                tx["gas"] = json!(adjusted_gas.as_u64());
                Some(adjusted_gas)
            },
            Ok(None) => None,
            Err(e) => {
                eprintln!("Error estimating gas limit: {}", e);
                None
            }
        }
    } else {
        gas_estimate.map(|g| U256::from_dec_str(g).unwrap_or_default())
    };

    if needs_gas_estimate && estimated_gas.is_none() {
        eprintln!("Gas estimation failed, aborting transaction formatting.");
        return Ok(json!(null));
    }

    // Use token details from params
    let from_token_details = params["fromTokenDetails"].as_object().ok_or("Missing fromTokenDetails")?;
    let to_token_details = params["toTokenDetails"].as_object().ok_or("Missing toTokenDetails")?;
    let native_token_details = params["nativeTokenDetails"].as_object().ok_or("Missing nativeTokenDetails")?; // Get the nativeTokenDetails

    let slippage = options["slippage"].as_f64().unwrap_or(1.0) / 100.0;
    let from_amount = U256::from_dec_str(amount).map_err(|e| format!("Invalid amount: {}", e))?;

    let gas_price_wei = U256::from_dec_str(gas_data[0].as_str().unwrap_or("0"))
        .map_err(|e| format!("Invalid gas price: {}", e))?;
    let gas_gwei = gas_data[1].as_str().unwrap_or("none");
    let gas_estimated = estimated_gas.map_or("0".to_string(), |gas| gas.to_string());

    let swap_cost_eth = estimated_gas.map_or("none".to_string(), |gas| {
        format!("{:.8}", (gas * gas_price_wei).as_u128() as f64 / 1e18)
    });

    let from_amount_usd = from_token_details["priceUSD"].as_f64().map(|price| {
        let amount_f64 = from_amount.as_u128() as f64 / 10f64.powi(from_token_details["decimals"].as_u64().unwrap_or(18) as i32);
        format!("{:.2}", amount_f64 * price)
    }).unwrap_or_else(|| "none".to_string());

    let to_amount_value = U256::from_dec_str(to_amount.as_str().unwrap_or("0")).unwrap_or_default();
    let to_amount_min: U256 = if dapp_options.and_then(|o| o["noSlippage"].as_bool()).unwrap_or(false) {
        to_amount_value
    } else if to_amount.as_str() != Some("none") {
        let min_amount = (to_amount_value.as_u128() as f64 * (1.0 - slippage)).floor() as u128;
        U256::from(min_amount)
    } else {
        to_amount_value
    };

    // Update swap_cost_usd to use the price of the native token (nativeTokenDetails)
    let swap_cost_usd = if swap_cost_eth != "none" && !native_token_details["priceUSD"].is_null() {
        format!(
            "{:.3}",
            swap_cost_eth.parse::<f64>().unwrap_or(0.0) * native_token_details["priceUSD"].as_f64().unwrap_or(0.0)
        )
    } else {
        "none".to_string()
    };

    let result = json!({
        "tool": tool,
        "fromChainId": from_chain_id,
        "fromAmountUSD": from_amount_usd,
        "fromAmount": from_amount.to_string(),
        "fromAddress": from_address,
        "toAmount": to_amount_value.to_string(),
        "toAmountMin": to_amount_min.to_string(),
        "swapCostETH": swap_cost_eth,
        "swapCostUSD": swap_cost_usd, // This now uses nativeTokenDetails for price calculation
        "toChainId": to_chain_id,
        "toAmountUSD": to_token_details["priceUSD"].as_f64().map(|price| {
            let amount = to_amount_value.as_u128() as f64 / 10f64.powi(to_token_details["decimals"].as_u64().unwrap_or(18) as i32);
            format!("{:.2}", amount * price)
        }).unwrap_or_else(|| "none".to_string()),
        "fromToken": {
            "address": from_token_details["address"],
            "chainId": from_chain_id,
            "symbol": from_token_details["symbol"],
            "decimals": from_token_details["decimals"],
            "name": from_token_details["name"],
            "logoURI": from_token_details["logoURI"],
            "priceUSD": from_token_details["priceUSD"].as_f64().map(|p| p.to_string()).unwrap_or_else(|| "none".to_string())
        },
        "toToken": {
            "address": to_token_details["address"],
            "chainId": to_chain_id,
            "symbol": to_token_details["symbol"],
            "decimals": to_token_details["decimals"],
            "name": to_token_details["name"],
            "coinKey": to_token_details["coinKey"],
            "logoURI": to_token_details["logoURI"],
            "priceUSD": to_token_details["priceUSD"].as_f64().map(|p| p.to_string()).unwrap_or_else(|| "none".to_string())
        },
        "options": options,
        "toAddress": to_address,
        "approvalAddress": approval_address.as_str().unwrap_or("none"),
        "gasGwei": gas_gwei,
        "transaction": {
            "value": tx["value"].as_str().unwrap_or("0"),
            "to": tx["to"].as_str().unwrap_or("none"),
            "from": tx["from"].as_str().unwrap_or("none"),
            "data": tx["data"].as_str().unwrap_or("none"),
            "chainId": tx["chainId"].as_u64().unwrap_or(from_chain_id),
            "gasPrice": gas_price_wei.to_string(),
            "gas": gas_estimated.to_string()
        }
    });

    if let Some(fee) = additional_fee {
        let fee_result = json!({
            "symbol": from_token_details["symbol"],
            "decimals": from_token_details["decimals"],
            "amount": fee
        });
        let mut result_mut = result.as_object().unwrap().clone();
        result_mut.insert("additionalFee".to_string(), fee_result);
        return Ok(Value::Object(result_mut));
    }

    Ok(result)
}
