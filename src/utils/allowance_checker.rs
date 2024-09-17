// src/utils/allowance_check.rs
use ethers::prelude::*;
use ethers::types::{Address, U256};
use serde_json::Value;
use std::sync::Arc;
use std::str::FromStr;
use crate::load_resources::AppState;
use crate::utils::utils::get_random_rpc_proxy_provider;
use tracing::error;

// ERC20 ABI for allowance function
const TOKEN_ABI: &str = r#"[
    {
        "constant": true,
        "inputs": [
            {
                "name": "_owner",
                "type": "address"
            },
            {
                "name": "_spender",
                "type": "address"
            }
        ],
        "name": "allowance",
        "outputs": [
            {
                "name": "",
                "type": "uint256"
            }
        ],
        "type": "function"
    }
]"#;

pub async fn check_user_allowance(quote: &Value, state: &Arc<AppState>) -> Result<bool, String> {
    let approval_address = quote["approvalAddress"].as_str().ok_or("Missing approvalAddress")?;
    let from_token = &quote["fromToken"];
    let transaction = &quote["transaction"];
    let from_amount = quote["fromAmount"].as_str().ok_or("Missing fromAmount")?;
    let chain_id = from_token["chainId"].as_u64().ok_or("Invalid chainId")?;

    let from_token_address = from_token["address"].as_str().ok_or("Missing fromToken address")?;
    
    // Check if fromToken.address is zero address (native token)
    if from_token_address == "0x0000000000000000000000000000000000000000" {
        return Ok(true); // Assume native token always has enough allowance
    }

    let provider = get_random_rpc_proxy_provider(chain_id, &state.rpc_proxy_providers)
        .ok_or_else(|| format!("No RPC provider found for chain ID: {}", chain_id))?;

    let from_address = transaction["from"].as_str().ok_or("Missing from address")?;
    let approval_address = Address::from_str(approval_address)
        .map_err(|e| format!("Invalid approval address: {}", e))?;
    let token_address = Address::from_str(from_token_address)
        .map_err(|e| format!("Invalid token address: {}", e))?;
    let owner = Address::from_str(from_address)
        .map_err(|e| format!("Invalid from address: {}", e))?;

    let allowance: U256 = get_allowance(&provider, token_address, owner, approval_address).await
        .map_err(|e| format!("Failed to get allowance: {}", e))?;

    let required_amount = U256::from_dec_str(from_amount)
        .map_err(|e| format!("Invalid fromAmount: {}", e))?;

    Ok(allowance >= required_amount)
}

#[derive(Debug, thiserror::Error)]
enum AllowanceError {
    #[error("ABI parsing error: {0}")]
    AbiParseError(#[from] serde_json::Error),
    #[error("Contract error: {0}")]
    ContractError(#[from] ContractError<Provider<Http>>),
    #[error("ABI error: {0}")]
    AbiError(String),
}

async fn get_allowance(
    provider: &Arc<Provider<Http>>,
    token_address: Address,
    owner: Address,
    spender: Address,
) -> Result<U256, AllowanceError> {
    let abi: ethers::abi::Abi = serde_json::from_str(TOKEN_ABI)?;
    
    let contract = Contract::new(token_address, abi, Arc::clone(provider));
    let method = contract.method::<_, U256>("allowance", (owner, spender))
        .map_err(|e| AllowanceError::AbiError(e.to_string()))?;
    
    Ok(method.call().await?)
}