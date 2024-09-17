// src/utils/balance_checker.rs
use ethers::prelude::*;
use ethers::types::{Address, U256};
use std::sync::Arc;
use std::str::FromStr;
use crate::load_resources::AppState;
use crate::utils::utils::get_random_rpc_proxy_provider;

pub async fn check_user_balance(
    from_address: &str,
    token_address: &str,
    amount: &str,
    from_chain_id: u64,
    state: &Arc<AppState>
) -> Result<bool, String> {
    let from_address = Address::from_str(from_address)
        .map_err(|e| format!("Invalid from_address: {}", e))?;
    let token_address = Address::from_str(token_address)
        .map_err(|e| format!("Invalid token_address: {}", e))?;
    let amount = U256::from_dec_str(amount)
        .map_err(|e| format!("Invalid amount: {}", e))?;

    let provider = get_random_rpc_proxy_provider(from_chain_id, &state.rpc_proxy_providers)
        .ok_or_else(|| format!("No RPC provider found for chain ID: {}", from_chain_id))?;

    let balance = if token_address == Address::zero() {
        // Native token balance
        provider.get_balance(from_address, None).await
            .map_err(|e| format!("Failed to get native token balance: {}", e))?
    } else {
        // ERC-20 token balance
        let token_contract = Contract::new(token_address, ERC20_ABI.clone(), provider.clone());
        token_contract.method::<_, U256>("balanceOf", from_address).unwrap()
            .call().await
            .map_err(|e| format!("Failed to get ERC-20 token balance: {}", e))?
    };

    Ok(balance >= amount)
}

// ERC-20 ABI for the balanceOf function
lazy_static::lazy_static! {
    static ref ERC20_ABI: ethers::abi::Abi = {
        serde_json::from_str(r#"[
            {
                "constant": true,
                "inputs": [{"name": "_owner", "type": "address"}],
                "name": "balanceOf",
                "outputs": [{"name": "balance", "type": "uint256"}],
                "type": "function"
            }
        ]"#).unwrap()
    };
}