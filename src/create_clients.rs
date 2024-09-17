// src/create_clients.rs
use std::sync::Arc;
use dashmap::DashMap;
use ethers::providers::{Provider, Http};
use reqwest::Client;
use serde_json::Value;
use std::fs;
use url::Url;
use web3::transports::Http as Web3Http;
use web3::Web3;


// Type definitions
pub type ProxyClientMap = Arc<DashMap<String, Client>>;
pub type RpcProxyProviderMap = Arc<DashMap<(u64, String), Arc<Provider<Http>>>>;
pub type Web3RpcProxyProviderMap = Arc<DashMap<(u64, String), Arc<Web3<Web3Http>>>>;
pub type JsonRpcProxyProviderMap = Arc<DashMap<(u64, String), Arc<(Client, String)>>>;

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
    //pub jsonrpc_rpc_proxy_providers: JsonRpcProxyProviderMap,
    pub quote_cache: Arc<DashMap<String, Value>>,
}

// Function to create proxy clients
pub async fn create_proxy_clients() -> ProxyClientMap {
    let clients_map = Arc::new(DashMap::new());

    let proxy_file_path = "config/proxy.txt";
    let proxy_content = match fs::read_to_string(proxy_file_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Failed to read proxy.txt: {}", e);
            return clients_map;
        }
    };

    for line in proxy_content.lines() {
        if let Some((ip_port, username, password)) = parse_proxy_details(line) {
            let proxy_url = format!("http://{}:{}@{}", username, password, ip_port);

            match reqwest::Proxy::all(&proxy_url) {
                Ok(proxy) => {
                    match Client::builder().proxy(proxy).build() {
                        Ok(client) => {
                            clients_map.insert(ip_port.clone(), client);
                        }
                        Err(e) => eprintln!("Failed to create reqwest client for {}: {}", ip_port, e),
                    }
                }
                Err(e) => eprintln!("Failed to create proxy for {}: {}", ip_port, e),
            }
        } else {
            eprintln!("Invalid proxy format in line: {}", line);
        }
    }

    clients_map
}

// Function to parse proxy details from a line
fn parse_proxy_details(line: &str) -> Option<(String, &str, &str)> {
    let parts: Vec<&str> = line.split(':').collect();
    if parts.len() == 4 {
        let ip_port = format!("{}:{}", parts[0], parts[1]);
        let username = parts[2];
        let password = parts[3];
        Some((ip_port, username, password))
    } else {
        None
    }
}

// Function to create RPC proxy providers
pub fn create_rpc_proxy_providers(
    chains: &Value,
    proxy_clients: &ProxyClientMap
) -> (
    RpcProxyProviderMap,
    //Web3RpcProxyProviderMap,
    JsonRpcProxyProviderMap,
) {
    let ethers_providers = Arc::new(DashMap::new());
    //let web3_providers = Arc::new(DashMap::new());
    let jsonrpc_providers = Arc::new(DashMap::new());

    if let Some(chain_list) = chains["chains"].as_array() {
        for chain in chain_list {
            if let (Some(chain_id), Some(rpc_urls)) = (chain["id"].as_u64(), chain["metamask"]["rpcUrls"].as_array()) {
                for rpc_url_str in rpc_urls.iter().filter_map(|url| url.as_str()) {
                    let rpc_url = match Url::parse(rpc_url_str) {
                        Ok(url) => url,
                        Err(_) => continue,
                    };

                    for entry in proxy_clients.iter() {
                        let proxy_id = entry.key();
                        let proxy_client = entry.value();

                        // Setup ethers provider
                        let ethers_provider = Provider::new(Http::new_with_client(rpc_url.clone(), proxy_client.clone()));
                        ethers_providers.insert((chain_id, proxy_id.clone()), Arc::new(ethers_provider));

                        /*/ Setup web3 provider
                        let web3_http = Web3Http::with_client(proxy_client.clone(), rpc_url.clone());
                        let web3 = Web3::new(web3_http);
                        web3_providers.insert((chain_id, proxy_id.clone()), Arc::new(web3));
                        */    
                        // Setup raw JSON-RPC provider
                        jsonrpc_providers.insert(
                            (chain_id, rpc_url.to_string()),
                            Arc::new((proxy_client.clone(), rpc_url.to_string()))
                        );
                    }
                }
            }
        }
    }
    //web3_providers
    (ethers_providers, jsonrpc_providers)
}