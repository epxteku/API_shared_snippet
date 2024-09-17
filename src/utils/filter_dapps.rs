use crate::load_resources::AppState;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

fn create_chain_name_to_id_map(rpc_config: &Value) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    if let Some(rpc_entries) = rpc_config.as_object() {
        for (id, details) in rpc_entries {
            if let Some(chain_name) = details.get("name").and_then(|v| v.as_str()) {
                if let Ok(id_number) = id.parse::<u64>() {
                    map.insert(chain_name.to_string(), id_number);
                }
            }
        }
    }
    map
}

fn resolve_chain_ids(chain_identifiers: &[Value], chain_name_to_id_map: &HashMap<String, u64>) -> Vec<u64> {
    chain_identifiers
        .iter()
        .filter_map(|id_or_name| {
            if let Some(chain_id) = id_or_name.as_u64() {
                Some(chain_id)
            } else if let Some(chain_name) = id_or_name.as_str() {
                chain_name_to_id_map.get(chain_name).cloned()
            } else {
                None
            }
        })
        .collect()
}

pub fn filter_dapps(
    token_address: &str,
    from_chain_id: u64,
    to_chain_id: u64,
    state: Arc<AppState>,
) -> Vec<String> {
    let dapp_config = &state.dapp_config;
    let rpc_config = &state.rpc_config;

    let chain_name_to_id_map = create_chain_name_to_id_map(&rpc_config);

    dapp_config
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(name, config)| {
            let enabled = config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            if !enabled {
                println!("{} is disabled.", name);
                return None;
            }

            // Check if DApp supports the token
            let supports_token = config
                .get("tokens")
                .and_then(|tokens| tokens.as_array())
                .map(|tokens| {
                    tokens.iter().any(|token| {
                        token.as_str() == Some("all")
                            || token.as_str() == Some(token_address)
                            || config.get("ethAddress").map_or(false, |v| v.as_str() == Some(token_address))
                    })
                })
                .unwrap_or(false);

            // Resolve chain IDs from names or IDs
            let from_chain_ids = config
                .get("fromChainIds")
                .and_then(|v| v.as_array())
                .map(|ids| resolve_chain_ids(ids, &chain_name_to_id_map))
                .unwrap_or_default();

            let to_chain_ids = config
                .get("toChainIds")
                .and_then(|v| v.as_array())
                .map(|ids| resolve_chain_ids(ids, &chain_name_to_id_map))
                .unwrap_or_default();

            // Check if "all" is present, meaning support for any chain
            let supports_any_from_chain = config
                .get("fromChainIds")
                .and_then(|v| v.as_array())
                .map_or(false, |ids| ids.iter().any(|id| id == "all"));

            let supports_any_to_chain = config
                .get("toChainIds")
                .and_then(|v| v.as_array())
                .map_or(false, |ids| ids.iter().any(|id| id == "all"));

            let supports_from_chain = supports_any_from_chain || from_chain_ids.contains(&from_chain_id);
            let supports_to_chain = supports_any_to_chain || to_chain_ids.contains(&to_chain_id);

            // Handle "bridge" setting
            match config.get("bridge").and_then(|v| v.as_bool()) {
                Some(true) => {
                    // Bridge mode: Chains can be different
                    if supports_token && supports_from_chain && supports_to_chain {
                        Some(name.clone())
                    } else {
                        None
                    }
                },
                Some(false) => {
                    // Non-bridge mode: fromChainId must equal toChainId
                    if supports_token && supports_from_chain && supports_to_chain && from_chain_id == to_chain_id {
                        Some(name.clone())
                    } else {
                        None
                    }
                },
                None => {
                    // Default behavior, assume bridge mode (chains can differ)
                    if supports_token && supports_from_chain && supports_to_chain {
                        Some(name.clone())
                    } else {
                        None
                    }
                }
            }
        })
        .collect()
}
