//src/paths/validate_params.rs
use std::collections::HashMap;
use serde_json::Value;
use regex::Regex;
use crate::load_resources::AppState;
use crate::paths::quote::QuoteParams;

pub struct ValidationResult {
    pub valid: bool,
    pub message: String,
}

pub fn validate_required_params(params: &QuoteParams, state: &AppState) -> ValidationResult {
    let mandatory_params = vec![
        "fromChainId",
        "fromAddress",
        "amount",
        "fromTokenAddress",
        "toTokenAddress",
    ];

    for param in &mandatory_params {
        if match *param {
            "fromChainId" if params.from_chain_id == 0 => true,
            "fromAddress" if params.from_address.is_empty() => true,
            "amount" if params.amount.is_empty() => true,
            "fromTokenAddress" if params.from_token_address.is_empty() => true,
            "toTokenAddress" if params.to_token_address.is_empty() => true,
            _ => false,
        } {
            return ValidationResult {
                valid: false,
                message: format!("Missing mandatory parameter: {}", param),
            };
        }
    }

    let chains = state.chains["chains"].as_array().unwrap();
    let is_valid_chain_id = |id: u32| chains.iter().any(|chain| {
        chain["id"].as_u64().map(|v| v == id as u64).unwrap_or(false)
    });
    
    if !is_valid_chain_id(params.from_chain_id) {
        return ValidationResult {
            valid: false,
            message: "Invalid fromChainId".to_string(),
        };
    }

    if let Some(to_chain_id) = params.to_chain_id {
        if !is_valid_chain_id(to_chain_id) {
            return ValidationResult {
                valid: false,
                message: "Invalid toChainId".to_string(),
            };
        }
    }

    let address_regex = Regex::new(r"^0x[a-fA-F0-9]{40}$").unwrap();
    if !address_regex.is_match(&params.from_token_address)
        || !address_regex.is_match(&params.to_token_address)
        || !address_regex.is_match(&params.from_address)
    {
        return ValidationResult {
            valid: false,
            message: "Invalid address format".to_string(),
        };
    }

    ValidationResult {
        valid: true,
        message: "Valid parameters".to_string(),
    }
}

pub fn safe_number_conversion(value: &str, default_value: f64) -> f64 {
    value.parse().unwrap_or(default_value)
}

pub fn format_options(params: &HashMap<String, Vec<String>>) -> Value {
    let mut formatted_options = serde_json::Map::new();
    
    // Handle slippage
    if let Some(slippage_vec) = params.get("slippage") {
        if let Some(slippage_str) = slippage_vec.first() {
            let slippage = slippage_str.parse::<f64>().unwrap_or(1.0);
            formatted_options.insert("slippage".to_string(), Value::Number(serde_json::Number::from_f64(slippage).unwrap()));
        }
    } else {
        formatted_options.insert("slippage".to_string(), Value::Number(serde_json::Number::from_f64(1.0).unwrap()));
    }

    // Handle dapps (splitting comma-separated string into array)
    if let Some(dapps_vec) = params.get("dapps") {
        if let Some(dapps_str) = dapps_vec.first() {
            let dapps_array: Vec<Value> = dapps_str
                .split(',')
                .map(|s| Value::String(s.trim().to_string()))
                .collect();
            formatted_options.insert("dapps".to_string(), Value::Array(dapps_array));
        }
    }

    // Handle other parameters
    for (key, values) in params {
        if key != "slippage" && key != "dapps" && !values.is_empty() {
            if values.len() == 1 {
                formatted_options.insert(key.clone(), Value::String(values[0].clone()));
            } else {
                formatted_options.insert(key.clone(), Value::Array(values.iter().cloned().map(Value::String).collect()));
            }
        }
    }

    Value::Object(formatted_options)
}
