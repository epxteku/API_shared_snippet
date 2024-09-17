use ethers::abi::{Function, Param, ParamType};
use ethers::prelude::*;
use ethers::types::{TransactionRequest, U256, Address, Bytes, U64};
use ethers::types::transaction::eip2718::TypedTransaction;
use serde_json::Value;
use std::sync::Arc;
use std::str::FromStr;
use crate::load_resources::AppState;
use crate::utils::get_random_rpc_proxy_provider;
use tracing::debug;

pub async fn generate_approval_transaction(
    quote: &Value,
    state: Arc<AppState>,
) -> Result<Option<TypedTransaction>, Box<dyn std::error::Error>> {
    let approval_address = quote["approvalAddress"].as_str().unwrap();
    let from_token = &quote["fromToken"];
    let transaction = &quote["transaction"];
    let approve = quote["approve"].as_str().unwrap_or("max");
    let from_amount = U256::from_dec_str(quote["fromAmount"].as_str().unwrap()).unwrap();
    let chain_id = from_token["chainId"].as_u64().unwrap();

    let provider = get_random_rpc_proxy_provider(chain_id, &state.rpc_proxy_providers)
        .ok_or("No provider available")?;

    let max_value = U256::MAX;
    let approval_amount = match approve {
        "max" => max_value,
        "value" => from_amount,
        _ => max_value,
    };

    let from_token_address = from_token["address"].as_str().unwrap();
    if from_token_address == "0x0000000000000000000000000000000000000000" {
        tracing::info!("fromToken.address is zero address, returning 'none'");
        return Ok(None);
    }

    // Define the allowance function
    let allowance_function = Function {
        name: "allowance".to_owned(),
        inputs: vec![
            Param {
                name: "owner".to_owned(),
                kind: ParamType::Address,
            },
            Param {
                name: "spender".to_owned(),
                kind: ParamType::Address,
            },
        ],
        outputs: vec![Param {
            name: "remaining".to_owned(),
            kind: ParamType::Uint(256),
        }],
        constant: Some(true),
        state_mutability: StateMutability::View,
    };

    let owner = transaction["from"].as_str().unwrap().parse::<Address>()?;
    let spender = approval_address.parse::<Address>()?;

    let allowance_data = allowance_function.encode_input(&[
        owner.into(),
        spender.into(),
    ])?;

    let call = provider
        .call(&TypedTransaction::Legacy(TransactionRequest {
            to: Some(from_token_address.parse()?),
            data: Some(Bytes::from(allowance_data)),
            ..Default::default()
        }), None)
        .await?;

    let allowance_amount = U256::from_big_endian(&call);
    if allowance_amount >= from_amount {
        tracing::info!("Allowance is sufficient, returning 'none'");
        return Ok(None);
    }

    // Define the approve function
    let approve_function = Function {
        name: "approve".to_owned(),
        inputs: vec![
            Param {
                name: "spender".to_owned(),
                kind: ParamType::Address,
            },
            Param {
                name: "amount".to_owned(),
                kind: ParamType::Uint(256),
            },
        ],
        outputs: vec![],
        constant: None,
        state_mutability: StateMutability::NonPayable,
    };

    let approve_data = approve_function.encode_input(&[
        spender.into(),
        approval_amount.into(),
    ])?;

    // Create the approval transaction
    let tx_request = TransactionRequest {
        from: Some(owner),
        to: Some(from_token_address.parse()?),
        data: Some(Bytes::from(approve_data)),
        value: Some(U256::zero()),
        gas_price: Some(U256::from_dec_str(transaction["gasPrice"].as_str().unwrap())?),
        chain_id: Some(chain_id.into()),
        ..Default::default()
    };

    let typed_tx = TypedTransaction::Legacy(tx_request);

    // Estimate gas
    match provider.estimate_gas(&typed_tx, None).await {
        Ok(gas) => Ok(Some(TypedTransaction::Legacy(TransactionRequest {
            gas: Some(gas),
            ..tx_request
        }))),
        Err(e) => {
            tracing::error!("Error estimating gas: {}", e);
            Ok(None)
        }
    }
}
