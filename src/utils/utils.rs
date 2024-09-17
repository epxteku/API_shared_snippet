// src/utils/utils.rs
use crate::load_resources::AppState;
use crate::create_clients::{
    ProxyClientMap, RpcProxyProviderMap, Web3RpcProxyProviderMap, JsonRpcProxyProviderMap
};
use web3::transports::Http as Web3Http;
use web3::Web3;
use ethers::providers::{Provider, Http, Middleware};
use reqwest::Client;
use rand::thread_rng;
use rand::prelude::IteratorRandom;
use std::sync::Arc;
use dashmap::DashMap;
use tracing::{error, debug};
use serde_json::Value;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::{Bytes, H160};
use std::str::FromStr;



pub async fn call_json_rpc(chain_id: u64, to: &str, data: &[u8], providers: &RpcProxyProviderMap) -> Result<Bytes, String> {
    let provider = get_random_rpc_proxy_provider(chain_id, providers)
        .ok_or_else(|| format!("Failed to get provider for chain ID: {}", chain_id))?;

    let to_address = H160::from_str(to).map_err(|e| format!("Invalid 'to' address: {}", e))?;
    let call_data = Bytes::from(data.to_vec());

    let tx = TypedTransaction::Eip1559(ethers::types::Eip1559TransactionRequest {
        to: Some(ethers::types::NameOrAddress::Address(to_address)),
        data: Some(call_data),
        ..Default::default()
    });

    provider.call(&tx, None)
        .await
        .map_err(|e| format!("RPC call failed: {}", e))
}

// Precomputation functions
pub fn precompute_proxy_clients(clients: &ProxyClientMap) -> ProxyClientMap {
    clients.clone()
}

pub fn precompute_chain_providers<T: Clone>(providers: &Arc<DashMap<(u64, String), Arc<T>>>) -> Arc<DashMap<(u64, String), Arc<T>>> {
    providers.clone()
}

// Updated functions to maintain original signatures
pub fn get_random_proxy_client(clients: &ProxyClientMap) -> Option<Client> {
    clients.iter().choose(&mut thread_rng()).map(|entry| entry.value().clone())
}

pub fn get_random_rpc_proxy_provider(
    chain_id: u64,
    rpc_proxy_providers: &RpcProxyProviderMap
) -> Option<Arc<Provider<Http>>> {
    rpc_proxy_providers
        .iter()
        .filter(|entry| entry.key().0 == chain_id)
        .choose(&mut thread_rng())
        .map(|entry| Arc::clone(entry.value()))
}

pub fn get_random_web3_proxy_provider(
    chain_id: u64,
    web3_rpc_proxy_providers: &Web3RpcProxyProviderMap
) -> Option<Arc<Web3<Web3Http>>> {
    web3_rpc_proxy_providers
        .iter()
        .filter(|entry| entry.key().0 == chain_id)
        .choose(&mut thread_rng())
        .map(|entry| Arc::clone(entry.value()))
}

pub fn get_random_jsonrpc_proxy_provider(chain_id: u64, providers: &JsonRpcProxyProviderMap) -> Option<(Arc<Client>, String)> {
    providers
        .iter()
        .filter(|entry| entry.key().0 == chain_id)
        .choose(&mut rand::thread_rng())
        .map(|entry| {
            let (client, rpc_url) = entry.value().as_ref();
            (Arc::new(client.clone()), rpc_url.clone())
        })
}

pub async fn fetch_gas_price(
    chain_id: u64,
    state: Arc<AppState>,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let max_retries = 10;
    let mut attempts = 0;

    while attempts < max_retries {
        // Attempt to select a random provider
        if let Some(provider) = get_random_rpc_proxy_provider(chain_id, &state.rpc_proxy_providers) {
            tracing::info!("Attempting to fetch gas price for chain ID {} (attempt {})", chain_id, attempts + 1);
            
            // Attempt to fetch the gas price from the provider
            match provider.get_gas_price().await {
                Ok(gas_price) => {
                    let gas_price_wei = gas_price.to_string();
                    let gas_price_gwei = format!("{:.9}", gas_price.as_u64() as f64 / 1e9);
                    tracing::info!("Successfully fetched gas price for chain ID {}: {} wei, {} gwei", chain_id, gas_price_wei, gas_price_gwei);
                    return Ok((gas_price_wei, gas_price_gwei));
                }
                Err(e) => {
                    tracing::error!("Failed to fetch gas price from provider on chain ID {}: attempt {}: {}", chain_id, attempts + 1, e);
                    attempts += 1;
                }
            }
        } else {
            tracing::error!("No provider available for chain ID {}", chain_id);
            break;
        }
    }

    tracing::error!("Failed to fetch gas price after {} attempts for chain ID {}", max_retries, chain_id);
    Ok(("0".to_string(), "0".to_string()))
}

pub fn fetch_dapp_config(dapp_name: &str, state: &AppState) -> Result<Value, Box<dyn std::error::Error>> {
    debug!("Fetching dapp config for: {}", dapp_name);
    if let Some(dapp_config) = state.dapp_config.get(dapp_name) {
        Ok(dapp_config.clone())
    } else {
        let error_msg = format!("DApp config for {} not found in state.dapp_config", dapp_name);
        error!("{}", error_msg);
        Err(error_msg.into())
    }
}

pub fn get_replaced_addresses(
    from_token_address: &str,
    to_token_address: &str,
    from_chain_id: u64,
    to_chain_id: u64,
    dapp_name: &str,
    state: &AppState
) -> Result<(String, String), Box<dyn std::error::Error>> {
    debug!("get_replaced_addresses called with: from_token={}, to_token={}, from_chain={}, to_chain={}, dapp={}",
           from_token_address, to_token_address, from_chain_id, to_chain_id, dapp_name);

    let dapp_config = match fetch_dapp_config(dapp_name, state) {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to fetch dapp config for {}: {}", dapp_name, e);
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::NotFound, format!("Dapp config not found: {}", e))));
        }
    };
    debug!("Fetched dapp_config: {:?}", dapp_config);

    let zero_address = "0x0000000000000000000000000000000000000000";
    let eee_address = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    let mut from_eth_address = dapp_config["ethAddress"].as_str().unwrap_or_else(|| {
        error!("ethAddress not found in dapp config, using zero address");
        zero_address
    }).to_string();
    let mut to_eth_address = from_eth_address.clone();

    debug!("Initial ETH addresses: from={}, to={}", from_eth_address, to_eth_address);

    // Check for chain-specific ETH addresses
    if let Some(chains) = dapp_config.get("chains") {
        debug!("Found 'chains' in dapp_config: {:?}", chains);

        if let Some(from_chain) = chains.get(&from_chain_id.to_string()) {
            debug!("Found config for from_chain_id {}: {:?}", from_chain_id, from_chain);
            from_eth_address = from_chain["ethAddress"].as_str().unwrap_or_else(|| {
                error!("ethAddress not found for from_chain_id {}, using previous value", from_chain_id);
                &from_eth_address
            }).to_string();
        } else {
            debug!("No specific config found for from_chain_id {}", from_chain_id);
        }

        if let Some(to_chain) = chains.get(&to_chain_id.to_string()) {
            debug!("Found config for to_chain_id {}: {:?}", to_chain_id, to_chain);
            to_eth_address = to_chain["ethAddress"].as_str().unwrap_or_else(|| {
                error!("ethAddress not found for to_chain_id {}, using previous value", to_chain_id);
                &to_eth_address
            }).to_string();
        } else {
            debug!("No specific config found for to_chain_id {}", to_chain_id);
        }
    } else {
        debug!("No 'chains' field found in dapp_config");
    }

    debug!("After chain-specific lookup: from_eth_address={}, to_eth_address={}", from_eth_address, to_eth_address);

    let from_token = if from_token_address == zero_address || from_token_address == eee_address {
        debug!("from_token_address is zero or eee, using from_eth_address");
        from_eth_address
    } else {
        debug!("Using original from_token_address");
        from_token_address.to_string()
    };

    let to_token = if to_token_address == zero_address || to_token_address == eee_address {
        debug!("to_token_address is zero or eee, using to_eth_address");
        to_eth_address
    } else {
        debug!("Using original to_token_address");
        to_token_address.to_string()
    };

    debug!("Final replaced addresses: from_token={}, to_token={}", from_token, to_token);

    Ok((from_token, to_token))
}

pub fn get_rpc_url(chain_id: u64, state: &AppState) -> Option<String> {
    let chains = &state.chains["chains"];
    if let Some(chain) = chains.as_array().and_then(|chains| chains.iter().find(|c| c["id"].as_u64() == Some(chain_id))) {
        chain["metamask"]["rpcUrls"].as_array()?.get(0)?.as_str().map(|s| s.to_string())
    } else {
        None
    }
}


pub fn serialize_big_ints(data: &mut Value) {
    match data {
        Value::Object(map) => {
            for (_, value) in map.iter_mut() {
                serialize_big_ints(value);
            }
        }
        Value::Number(num) if num.is_i64() || num.is_u64() => {
            let num_str = num.to_string();
            *data = Value::String(num_str);
        }
        _ => {}
    }
}