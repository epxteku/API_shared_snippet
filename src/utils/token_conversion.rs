use rust_decimal::Decimal;
use std::str::FromStr;

pub fn format_units(amount: u128, decimals: u8) -> Result<String, String> {
    let scale = 10u128.pow(decimals as u32);
    let decimal_amount = Decimal::from_str(&amount.to_string())
        .map_err(|e| e.to_string())?
        / Decimal::from(scale);
    
    Ok(decimal_amount.to_string())
}

pub fn parse_units(amount_str: &str, decimals: u8) -> Result<String, String> {
    let decimal_amount = Decimal::from_str(amount_str)
        .map_err(|e| e.to_string())?;
    let scale = Decimal::from(10u128.pow(decimals as u32));
    let scaled_amount = decimal_amount * scale;
    
    Ok(scaled_amount.to_string().split('.').next().unwrap_or("0").to_string())
}
