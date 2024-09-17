use serde::ser::{Serialize, Serializer, SerializeMap};
use serde_json::Value;
use indexmap::IndexMap;

pub struct OrderedValue(IndexMap<String, Value>);

impl OrderedValue {
    pub fn new() -> Self {
        OrderedValue(IndexMap::new())
    }

    pub fn insert(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_string(), value);
    }
}

impl Serialize for OrderedValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

pub fn create_ordered_response(response: &Value) -> OrderedValue {
    let mut ordered = OrderedValue::new();
    ordered.insert("requestId", response["requestId"].clone());
    ordered.insert("success", response["success"].clone());
    ordered.insert("data", response["data"].clone());
    ordered
}

pub fn serialize_ordered_response(response: &Value) -> Value {
    let ordered = create_ordered_response(response);
    serde_json::to_value(ordered).unwrap_or(Value::Null)
}