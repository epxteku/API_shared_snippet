// src/dapps/across.rs
use ethers::abi::{Abi, Token, Function};
use ethers::types::{U256, Address, Bytes};
use ethers::utils::hex::encode as to_hex;
use ethers::contract::AbiError;
use serde_json::{Value, json};
use std::fs;
use std::sync::Arc;
use std::path::PathBuf;
use std::str::FromStr;
use futures::future::BoxFuture;
use futures::FutureExt;
use crate::utils::utils::{get_replaced_addresses, get_random_proxy_client};
use crate::utils::format_swap_details::format_swap_details;
use crate::load_resources::AppState;
use tracing::{debug, error};
use url::Url;

pub struct AcrossAbi {
    pub abi: Abi,
}

impl AcrossAbi {
    pub fn new(abi_path: PathBuf) -> Self {
        let abi_content = fs::read_to_string(abi_path).expect("Failed to load ABI");
        let abi: Abi = serde_json::from_str(&abi_content).expect("Invalid ABI format");
        Self { abi }
    }
}

// Load ABI once
lazy_static::lazy_static! {
    static ref ACROSS_ABI: AcrossAbi = AcrossAbi::new(PathBuf::from("./src/dapps/abi/across/abi.json"));
}

pub fn get_swap_quote(params: Value, state: Arc<AppState>) -> BoxFuture<'static, Result<Value, String>> {
    async move {
        debug!("Starting get_swap_quote with params: {:?}", params);

        let from_chain_id = params["fromChainId"].as_u64().ok_or("Invalid fromChainId")?;
        let amount = params["amount"].as_str().ok_or("Invalid amount")?;
        let from_token_address = params["fromTokenAddress"].as_str().ok_or("Invalid fromTokenAddress")?;
        let to_token_address = params["toTokenAddress"].as_str().ok_or("Invalid toTokenAddress")?;
        let to_address = params["toAddress"].as_str().ok_or("Invalid toAddress")?;
        let to_chain_id = params["toChainId"].as_u64().ok_or("Invalid toChainId")?;
        let from_address = params["fromAddress"].as_str().ok_or("Invalid fromAddress")?;
        let quote_only = params["quoteOnly"].as_bool().unwrap_or(false);

        debug!("Extracted parameters: from_chain_id={}, to_chain_id={}, amount={}, quote_only={}", 
               from_chain_id, to_chain_id, amount, quote_only);

        let (valid_from_token_address, _valid_to_token_address) = get_replaced_addresses(
            from_token_address,
            to_token_address,
            from_chain_id,
            to_chain_id,
            "across",
            &state
        ).map_err(|e| format!("Failed to replace addresses: {}", e))?;
        debug!("Replaced addresses: valid_from_token_address={}", valid_from_token_address);

        let full_quote_result = fetch_data_and_calculate(
            &valid_from_token_address,
            to_chain_id,
            amount,
            from_chain_id,
            to_address,
            &state
        ).await?;
        debug!("Fetched full quote result: {:?}", full_quote_result);

        if !full_quote_result["success"].as_bool().unwrap_or(false) {
            error!("Failed to retrieve or process quote: {:?}", full_quote_result);
            return Err(full_quote_result["message"].as_str().unwrap_or("Unable to retrieve or process quote").to_string());
        }

        let zero_address = "0x0000000000000000000000000000000000000000";
        let eee_address = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let value = if from_token_address == zero_address || from_token_address == eee_address {
            amount.to_string()
        } else {
            "0".to_string()
        };

        let transaction_data = full_quote_result["data"]["transactionData"].clone();
        let amount_out = full_quote_result["data"]["amountOut"].clone();
        let spoke_pool_address = full_quote_result["data"]["spokePoolAddress"].clone();

        let formatted_result = if quote_only {
            let transaction_data_basic = json!({
                "from": from_address,
                "to": spoke_pool_address,
                "chainID": from_chain_id,
                "data": transaction_data,
                "value": value,
            });
            debug!("Transaction data for quote: {:?}", transaction_data_basic);

            format_swap_details(
                "across",
                &params,
                &transaction_data_basic,
                &amount_out,
                &spoke_pool_address,
                &params["gasPrices"],
                Some("quote"),
                None,
                None,
                &state
            )
            .await
        } else {
            let transaction_data_full = json!({
                "from": from_address,
                "to": spoke_pool_address,
                "chainID": from_chain_id,
                "data": transaction_data,
                "value": value,
            });
            debug!("Transaction data for full swap: {:?}", transaction_data_full);

            format_swap_details(
                "across",
                &params,
                &transaction_data_full,
                &amount_out,
                &spoke_pool_address,
                &params["gasPrices"],
                None,
                None,
                None,
                &state
            )
            .await
        };

        match &formatted_result {
            Ok(result) => debug!("Formatted swap details result: {:?}", result),
            Err(e) => error!("Error formatting swap details: {}", e),
        }

        formatted_result.map_err(|e| format!("Error formatting swap details: {}", e))
    }.boxed()
}

async fn fetch_suggested_fees(
    token: &str,
    destination_chain_id: u64,
    amount: &str,
    origin_chain_id: u64,
    recipient: &str,
    state: &Arc<AppState>
) -> Result<Value, String> {
    let mut url = Url::parse("https://app.across.to/api/suggested-fees").map_err(|e| e.to_string())?;

    url.query_pairs_mut()
        .append_pair("token", token)
        .append_pair("destinationChainId", &destination_chain_id.to_string())
        .append_pair("amount", amount)
        .append_pair("originChainId", &origin_chain_id.to_string())
        .append_pair("recipient", recipient);

    debug!("Fetching suggested fees with URL: {}", url);

    let client = get_random_proxy_client(&state.proxy_clients)
        .ok_or_else(|| {
            error!("No proxy client available");
            "No proxy client available".to_string()
        })?;

    let response = client.get(url) // Remove the & here
        .send()
        .await
        .map_err(|e| {
            error!("Failed to send request for suggested fees: {}", e);
            format!("Failed to send request: {}", e)
        })?;

    if response.status() != 200 {
        let error_message = format!("HTTP error! Status: {} {}", response.status(), response.status().canonical_reason().unwrap_or(""));
        error!("{}", error_message);
        let response_text = response.text().await.unwrap_or_else(|_| "Failed to get response body".to_string());
        error!("Response body: {}", response_text);
        return Err(error_message);
    }

    let json: Value = response.json().await.map_err(|e| {
        error!("Failed to parse suggested fees response: {}", e);
        format!("Failed to parse response: {}", e)
    })?;
    
    Ok(json)
}

async fn fetch_limits(
    token: &str,
    destination_chain_id: u64,
    origin_chain_id: u64,
    state: &Arc<AppState>
) -> Result<Value, String> {
    let mut url = Url::parse("https://app.across.to/api/limits").map_err(|e| e.to_string())?;

    url.query_pairs_mut()
        .append_pair("token", token)
        .append_pair("destinationChainId", &destination_chain_id.to_string())
        .append_pair("originChainId", &origin_chain_id.to_string());

    debug!("Fetching limits with URL: {}", url);

    let client = get_random_proxy_client(&state.proxy_clients)
        .ok_or_else(|| {
            error!("No proxy client available");
            "No proxy client available".to_string()
        })?;

    let response = client.get(url) // Remove the & here as well
        .send()
        .await
        .map_err(|e| {
            error!("Failed to send request for limits: {}", e);
            format!("Failed to send request: {}", e)
        })?;

    if response.status() != 200 {
        let error_message = format!("HTTP error! Status: {}", response.status());
        error!("{}", error_message);
        return Err(error_message);
    }

    let json: Value = response.json().await.map_err(|e| {
        error!("Failed to parse limits response: {}", e);
        format!("Failed to parse response: {}", e)
    })?;
    
    Ok(json)
}

async fn fetch_data_and_calculate(
    token: &str,
    destination_chain_id: u64,
    amount: &str,
    origin_chain_id: u64,
    recipient: &str,
    state: &Arc<AppState>
) -> Result<Value, String> {
    debug!("Fetching data and calculating: token={}, destination_chain_id={}, amount={}, origin_chain_id={}, recipient={}",
           token, destination_chain_id, amount, origin_chain_id, recipient);

    let suggested_fees = fetch_suggested_fees(token, destination_chain_id, amount, origin_chain_id, recipient, state).await
        .map_err(|e| format!("Failed to fetch suggested fees: {}", e))?;
    
    debug!("Suggested fees response: {:?}", suggested_fees);

    let suggested_fees = suggested_fees.as_object()
        .ok_or_else(|| "Suggested fees response is not an object".to_string())?;

    let relay_fee_total = suggested_fees.get("relayFeeTotal")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing relayFeeTotal".to_string())?;

    let spoke_pool_address = suggested_fees.get("spokePoolAddress")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing spokePoolAddress".to_string())?;

    let relay_fee_pct_str = suggested_fees.get("relayFeePct")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing relayFeePct".to_string())?;

    let timestamp_str = suggested_fees.get("timestamp")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing timestamp".to_string())?;

    let amount_u256 = U256::from_dec_str(amount)
        .map_err(|e| format!("Invalid amount: {}", e))?;
    let relay_fee_total_u256 = U256::from_dec_str(relay_fee_total)
        .map_err(|e| format!("Invalid relayFeeTotal: {}", e))?;

    let amount_out = (amount_u256 - relay_fee_total_u256).to_string();

    let limits = fetch_limits(token, destination_chain_id, origin_chain_id, state).await
        .map_err(|e| format!("Failed to fetch limits: {}", e))?;
    
    debug!("Limits response: {:?}", limits);

    let limits = limits.as_object()
        .ok_or_else(|| "Limits response is not an object".to_string())?;

    let min_deposit = limits.get("minDeposit")
        .and_then(|v| v.as_str())
        .unwrap_or("0");

    let max_deposit_instant = limits.get("maxDepositInstant")
        .and_then(|v| v.as_str())
        .unwrap_or("0");

    let min_deposit_u256 = U256::from_dec_str(min_deposit)
        .map_err(|e| format!("Invalid minDeposit: {}", e))?;
    let max_deposit_instant_u256 = U256::from_dec_str(max_deposit_instant)
        .map_err(|e| format!("Invalid maxDepositInstant: {}", e))?;

    let is_amount_within_limits = amount_u256 > min_deposit_u256 && amount_u256 < max_deposit_instant_u256;

    if !is_amount_within_limits {
        return Ok(json!({
            "success": false,
            "message": "Amount is out of limits."
        }));
    }

    // Convert the relay fee percentage and timestamp from &str to i64 and u32 respectively
    let relay_fee_pct: i64 = relay_fee_pct_str.parse().map_err(|_| "Invalid relay fee percentage")?;
    let timestamp: u32 = timestamp_str.parse().map_err(|_| "Invalid timestamp")?;

    // Pass the ABI instead of provider.clone()
    let transaction_data = match generate_transaction_data(
        recipient,
        token,
        amount,
        destination_chain_id,
        relay_fee_pct,
        timestamp,
        spoke_pool_address,
        &ACROSS_ABI.abi // Correct ABI passed here
    ).await {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to generate transaction data: {}", e);
            return Err(format!("Failed to generate transaction data: {}", e));
        }
    };

    Ok(json!({
        "success": true,
        "data": {
            "transactionData": transaction_data,
            "amountOut": amount_out,
            "spokePoolAddress": spoke_pool_address
        }
    }))
}
pub async fn generate_transaction_data(
    recipient: &str,
    origin_token: &str,
    amount: &str,
    destination_chain_id: u64,
    relayer_fee_pct: i64,
    quote_timestamp: u32,
    _spoke_pool_address: &str,
    abi: &Abi, // Pass the ABI here
) -> Result<String, AbiError> {


    // Parse recipient and origin token addresses
    let recipient_address = Address::from_str(recipient).expect("Invalid recipient address");
    let origin_token_address = Address::from_str(origin_token).expect("Invalid origin token address");

    // Parse amount to U256
    let amount_u256 = U256::from_dec_str(amount).expect("Invalid amount");

    // Get the "deposit" function from the ABI
    let function: &Function = abi.function("deposit")?;

    // Prepare the parameters for the contract method call
    let params = vec![
        Token::Address(recipient_address),
        Token::Address(origin_token_address),
        Token::Uint(amount_u256),
        Token::Uint(U256::from(destination_chain_id)),
        Token::Int(relayer_fee_pct.into()),
        Token::Uint(U256::from(quote_timestamp)),
        Token::Bytes(vec![]), // Empty message
        Token::Uint(U256::MAX), // Max uint256 value
    ];

    // Encode the function data for the "deposit" function
    let data: Bytes = function.encode_input(&params)?.into();

    // Convert the data to hex string in the "0x..." format
    let hex_data = format!("0x{}", to_hex(data));

    Ok(hex_data)
}