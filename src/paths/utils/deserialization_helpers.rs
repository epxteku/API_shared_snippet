use serde::de::{self, Deserialize, Deserializer, SeqAccess, Visitor};
use tracing::debug;
use std::fmt;

// Custom deserializer to handle both number and string inputs for u32
pub fn string_or_number_to_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;

    debug!("Deserializing value for u32: {:?}", value);

    match value {
        serde_json::Value::Number(num) => num.as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| de::Error::custom("Invalid number for u32")),
        serde_json::Value::String(s) => s.parse::<u32>()
            .map_err(|_| de::Error::custom("Invalid string for u32")),
        _ => Err(de::Error::custom("Expected a string or a number")),
    }
}

// Custom deserializer to ensure `amount` is always treated as a string
pub fn number_to_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;

    debug!("Deserializing value for string: {:?}", value);

    match value {
        serde_json::Value::Number(num) => Ok(num.to_string()),
        serde_json::Value::String(s) => Ok(s),
        _ => Err(de::Error::custom("Expected a string or a number")),
    }
}

// Custom deserializer to handle both number and string inputs for Option<u32>
pub fn string_or_number_to_option_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;

    debug!("Deserializing value for Option<u32>: {:?}", value);

    match value {
        serde_json::Value::Number(num) => num.as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| de::Error::custom("Invalid number for u32")),
        serde_json::Value::String(s) => s.parse::<u32>()
            .map(Some)
            .map_err(|_| de::Error::custom("Invalid string for u32")),
        serde_json::Value::Null => Ok(None),
        _ => Err(de::Error::custom("Expected a string, number, or null")),
    }
}

// Custom deserializer to handle both string and array inputs for Vec<String> (dapps)
pub fn string_or_seq<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrSeqVisitor;

    impl<'de> Visitor<'de> for StringOrSeqVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or a sequence of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Vec<String>, E>
        where
            E: de::Error,
        {
            Ok(vec![value.to_string()])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<String>, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                values.push(value);
            }
            Ok(values)
        }
    }

    deserializer.deserialize_any(StringOrSeqVisitor)
}

// Custom deserializer for slippage, handling both string and number inputs
pub fn string_or_number_to_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;

    match value {
        serde_json::Value::Number(num) => num.as_f64().ok_or_else(|| de::Error::custom("Invalid number for f64")),
        serde_json::Value::String(s) => s.parse::<f64>().map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom("Expected a number or string")),
    }
}
