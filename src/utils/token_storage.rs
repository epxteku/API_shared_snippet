use std::sync::Arc;
use dashmap::DashMap;
use serde_json::Value;
use crate::utils::optimized_token_lookup::{OptimizedTokenLookup, TokenInfo};

pub struct HybridTokenStorage {
    old_storage: Arc<DashMap<String, Value>>,
    new_storage: Arc<OptimizedTokenLookup>,
}

impl HybridTokenStorage {
    pub fn new() -> Self {
        Self {
            old_storage: Arc::new(DashMap::new()),
            new_storage: Arc::new(OptimizedTokenLookup::new()),
        }
    }

    // Methods for the old storage system
    pub fn insert_old(&self, key: String, value: Value) {
        self.old_storage.insert(key, value);
    }

    pub fn get_old(&self, key: &str) -> Option<Value> {
        self.old_storage.get(key).map(|v| v.clone())
    }

    // Methods for the new storage system
    pub fn insert_new(&self, chain_id: u64, address: String, token_info: TokenInfo) {
        self.new_storage.insert(chain_id, address, token_info);
    }

    pub fn get_new(&self, chain_id: u64, address: &str) -> Option<TokenInfo> {
        self.new_storage.get(chain_id, address)
    }

    // Method to access the optimized lookup directly
    pub fn optimized_lookup(&self) -> &Arc<OptimizedTokenLookup> {
        &self.new_storage
    }
}