use std::sync::Arc;
use ethers::{
    prelude::*,
    types::H160,
};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use crate::load_resources::AppState;
use crate::utils::utils::get_random_rpc_proxy_provider;
use std::path::Path;
use std::fs::OpenOptions;
use serde::de::{self, Deserializer}; // Import the Deserializer trait from serde::de

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub address: String,
    #[serde(rename = "chainId")]
    pub chain_id: u64,
    pub symbol: String,
    pub decimals: u8,
    pub name: String,
    #[serde(rename = "coinKey")]
    pub coin_key: String,
    #[serde(rename = "logoURI")]
    pub logo_uri: String,
    #[serde(rename = "priceUSD", deserialize_with = "deserialize_price_usd")]
    pub price_usd: Option<f64>,
}

fn deserialize_price_usd<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => s.parse::<f64>().map(Some).map_err(de::Error::custom),
        serde_json::Value::Number(num) => Ok(num.as_f64()), // Return Option<f64> directly
        _ => Ok(None),
    }
}

fn normalize_token_address(token_address: &str) -> String {
    let zero_address = "0x0000000000000000000000000000000000000000";
    let eeee_address = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    if token_address.to_lowercase() == eeee_address {
        zero_address.to_string()
    } else {
        token_address.to_lowercase()
    }
}

fn find_tokens_in_json(tokens: &Value, chain_id: u64, normalized_addresses: &[String]) -> Vec<Option<Value>> {
    normalized_addresses.iter().map(|address| {
        tokens.get("tokens")
            .and_then(|tokens| tokens.get(&chain_id.to_string()))
            .and_then(|chain_tokens| chain_tokens.as_array())
            .and_then(|tokens_array| {
                tokens_array.iter()
                    .find(|token| {
                        token["address"].as_str()
                            .map(|addr| addr.to_lowercase() == address.to_lowercase())
                            .unwrap_or(false)
                    })
                    .cloned()
            })
    }).collect()
}

pub async fn fetch_token_details(
    tokens_with_chain_ids: Vec<(&str, u64)>, 
    state: &Arc<AppState>
) -> Result<Vec<Option<TokenInfo>>, String> {
    // Normalize all addresses first
    let normalized_addresses: Vec<String> = tokens_with_chain_ids
        .iter()
        .map(|(token_address, _)| normalize_token_address(token_address))
        .collect();
    
    let chain_id = tokens_with_chain_ids[0].1;  // Assuming all tokens have the same chain ID
    let mut fetched_tokens: Vec<Option<TokenInfo>> = Vec::new();
    
    // Try finding all tokens in the JSON cache in one go
    let json_tokens = state.tokens.get("tokens").or_else(|| state.tokens.get("new_tokens"));

    if let Some(tokens) = json_tokens {
        let found_tokens = find_tokens_in_json(&tokens, chain_id, &normalized_addresses);
        
        for (_idx, token_value) in found_tokens.iter().enumerate() {
            if let Some(token_value) = token_value {
                // Parse token details from cache
                match serde_json::from_value::<TokenInfo>(token_value.clone()) {
                    Ok(token_info) => fetched_tokens.push(Some(token_info)),
                    Err(e) => return Err(format!("Failed to parse token info from cache: {}", e)),
                }
            } else {
                // If token is not found in JSON, push None (fetch from network later)
                fetched_tokens.push(None);
            }
        }
    } else {
        // If no cache, all tokens need to be fetched from the network
        fetched_tokens = vec![None; tokens_with_chain_ids.len()];
    }

    // Now fetch missing tokens (those with None) from the network one at a time
    for (idx, token_info) in fetched_tokens.iter_mut().enumerate() {
        if token_info.is_none() {
            let (_token_address, chain_id) = tokens_with_chain_ids[idx];
            let normalized_address = &normalized_addresses[idx];

            match fetch_token_from_network(normalized_address, chain_id, state).await {
                Ok(network_token_info) => {
                    // Save the fetched token in the cache
                    save_token_to_new_tokens(&network_token_info, state);
                    *token_info = Some(network_token_info);
                },
                Err(e) => return Err(format!("Failed to fetch token from network: {}", e)),
            }
        }
    }

    Ok(fetched_tokens)  // Return all found tokens, including None for missing tokens
}

async fn fetch_token_from_network(token_address: &str, chain_id: u64, state: &Arc<AppState>) -> Result<TokenInfo, String> {
    let provider = get_random_rpc_proxy_provider(chain_id, &state.rpc_proxy_providers)
        .ok_or_else(|| format!("No provider available for chain ID: {}", chain_id))?;

    let address = token_address.parse::<H160>().map_err(|e| format!("Invalid address: {}", e))?;

    abigen!(
        ERC20,
        r#"[
            {"constant":true,"inputs":[],"name":"symbol","outputs":[{"name":"","type":"string"}],"type":"function"},
            {"constant":true,"inputs":[],"name":"decimals","outputs":[{"name":"","type":"uint8"}],"type":"function"}
        ]"#
    );

    let token_contract = ERC20::new(address, Arc::new(provider));

    let symbol: String = token_contract.symbol().call().await
        .map_err(|e| format!("Failed to fetch symbol: {}", e))?;
    let decimals: u8 = token_contract.decimals().call().await
        .map_err(|e| format!("Failed to fetch decimals: {}", e))?;

    println!("Fetched token from network: {} (symbol: {}, decimals: {})", token_address, symbol, decimals);

    Ok(TokenInfo {
        address: token_address.to_string(),
        chain_id,
        symbol: symbol.clone(),
        decimals,
        name: symbol.clone(),
        coin_key: symbol,
        logo_uri: String::new(),
        price_usd: None,
    })
}

fn save_token_to_new_tokens(token_info: &TokenInfo, state: &Arc<AppState>) {
    let mut new_tokens = state.tokens.get("new_tokens")
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    let chain_id = token_info.chain_id.to_string();
    let chain_tokens = new_tokens.entry(chain_id.clone()).or_insert(Value::Array(Vec::new()));

    if let Value::Array(ref mut tokens) = chain_tokens {
        tokens.push(serde_json::to_value(token_info).unwrap());
    }

    let new_tokens_value = Value::Object(new_tokens);
    state.tokens.insert("new_tokens".to_string(), new_tokens_value.clone());

    println!("Saved new token to cache: {:?}", token_info);

    // Save new tokens to file
    if let Err(e) = save_token_to_file(&new_tokens_value) {
        eprintln!("Error saving new tokens to file: {}", e);
    }
}

fn save_token_to_file(new_tokens: &Value) -> Result<(), String> {
    let file_path = Path::new("src/config/new_tokens.json");

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true) // Clear the file before writing
        .open(file_path)
        .map_err(|e| format!("Failed to open new_tokens.json for writing: {}", e))?;

    serde_json::to_writer_pretty(&file, new_tokens)
        .map_err(|e| format!("Failed to write to new_tokens.json: {}", e))?;

    Ok(())
}
