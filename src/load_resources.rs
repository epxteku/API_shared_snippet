// src/load_resources.rs
use serde_json::Value;
use std::sync::Arc;
use std::fs;
use std::path::PathBuf;
use dashmap::DashMap;
use tokio::time::{interval, Duration};
use crate::create_clients::{
    create_proxy_clients, create_rpc_proxy_providers, ProxyClientMap, RpcProxyProviderMap, JsonRpcProxyProviderMap
};
use std::collections::HashMap;
use crate::utils::utils::{precompute_proxy_clients, precompute_chain_providers};

pub struct AppState {
    pub dapps: Value,
    pub chains: Value,
    pub tokens: Arc<DashMap<String, Value>>,
    pub dapp_config: Value,
    pub rpc_config: Value,
    pub settings: Value,
    pub proxy_clients: ProxyClientMap,
    pub rpc_proxy_providers: RpcProxyProviderMap,
    //pub web3_rpc_proxy_providers: Web3RpcProxyProviderMap,
    pub jsonrpc_rpc_proxy_providers: JsonRpcProxyProviderMap,
    pub quote_cache: Arc<DashMap<String, Value>>,
}

// Function to load JSON from a file
pub fn load_json(file_path: PathBuf) -> Result<Value, &'static str> {
    match fs::read_to_string(file_path) {
        Ok(file_content) => match serde_json::from_str::<Value>(&file_content) {
            Ok(json) => Ok(json),
            Err(_) => Err("Failed to parse JSON"),
        },
        Err(_) => Err("File not found"),
    }
}

// Function to create AppState with all RPC providers
pub async fn create_app_state() -> AppState {
    println!("Current working directory: {:?}", std::env::current_dir().unwrap());
    let dapps = load_json(PathBuf::from("./config/dapps.json")).expect("Failed to load dapps.json");
    let chains = load_json(PathBuf::from("./config/chains.json")).expect("Failed to load chains.json");
    let tokens = load_json(PathBuf::from("./config/tokens.json")).expect("Failed to load tokens.json");
    let dapp_config = load_json(PathBuf::from("./config/dappConfig.json")).expect("Failed to load dappConfig.json");
    let rpc_config = load_json(PathBuf::from("./config/rpc.json")).expect("Failed to load rpc.json");
    let settings = load_json(PathBuf::from("./config/settings.json")).expect("Failed to load settings.json");
    let quote_cache = Arc::new(DashMap::new());

    let tokens_map = Arc::new(DashMap::new());

    // Transform the token list into a map of "chainId:address" -> TokenInfo for fast lookup
    let mut token_lookup_map = HashMap::new();
    if let Some(tokens_obj) = tokens["tokens"].as_object() {
        for (chain_id, tokens_list) in tokens_obj {
            if let Some(tokens_array) = tokens_list.as_array() {
                for token in tokens_array {
                    if let Some(address) = token["address"].as_str() {
                        let key = format!("{}:{}", chain_id, address.to_lowercase());
                        token_lookup_map.insert(key, token.clone());
                    }
                }
            }
        }
    }
    tokens_map.insert("tokens".to_string(), serde_json::to_value(token_lookup_map).unwrap());

    // Create proxy clients
    let proxy_clients = create_proxy_clients().await;
    let precomputed_proxy_clients = precompute_proxy_clients(&proxy_clients);
    tracing::info!("Loaded {} proxy clients", precomputed_proxy_clients.len());

    // Load ethers, web3, and JSON-RPC providers //web3_rpc_proxy_providers
    let (rpc_proxy_providers, jsonrpc_rpc_proxy_providers) =
        create_rpc_proxy_providers(&chains, &precomputed_proxy_clients);
    
    let precomputed_rpc_providers = precompute_chain_providers(&rpc_proxy_providers);
    //let precomputed_web3_providers = precompute_chain_providers(&web3_rpc_proxy_providers);
    let precomputed_jsonrpc_providers = precompute_chain_providers(&jsonrpc_rpc_proxy_providers);

    tracing::info!("Loaded {} ethers RPC proxy providers", precomputed_rpc_providers.len());
    //tracing::info!("Loaded {} web3 RPC proxy providers", precomputed_web3_providers.len());
    tracing::info!("Loaded {} JSON-RPC proxy providers", precomputed_jsonrpc_providers.len());

    AppState {
        dapps,
        chains,
        tokens: tokens_map,
        dapp_config,
        rpc_config,
        settings,
        proxy_clients: precomputed_proxy_clients,
        rpc_proxy_providers: precomputed_rpc_providers,
        //web3_rpc_proxy_providers: precomputed_web3_providers,
        jsonrpc_rpc_proxy_providers: precomputed_jsonrpc_providers,
        quote_cache,
    }
}

// Function to periodically reload tokens.json
pub async fn reload_tokens(state: Arc<AppState>) {
    let file_path = PathBuf::from("./config/tokens.json");
    let mut interval = interval(Duration::from_secs(300));

    loop {
        interval.tick().await;

        match load_json(file_path.clone()) {
            Ok(new_tokens) => {
                state.tokens.insert("tokens".to_string(), new_tokens);
                //println!("Tokens updated in memory.");
            }
            Err(_) => {
                println!("Failed to reload tokens.json");
            }
        }
    }
}