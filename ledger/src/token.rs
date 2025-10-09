use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;

/// Simple emission schedule enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Emission {
    /// Fixed total supply minted at genesis
    Fixed(u64),
    /// Linear emission with `initial` supply and `rate` per block
    Linear { initial: u64, rate: u64 },
}

impl Emission {
    pub fn supply_at(&self, height: u64) -> u64 {
        match self {
            Emission::Fixed(v) => *v,
            Emission::Linear { initial, rate } => {
                initial.saturating_add(rate.saturating_mul(height))
            }
        }
    }
}

/// Information about a registered token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub symbol: String,
    pub emission: Emission,
}

/// Registry for native tokens with pluggable emission schedules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenRegistry {
    tokens: HashMap<String, TokenInfo>,
}

impl TokenRegistry {
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
        }
    }

    /// Register a new token. Returns `false` if it already exists.
    pub fn register(&mut self, symbol: &str, emission: Emission) -> bool {
        let info = TokenInfo {
            symbol: symbol.to_string(),
            emission,
        };
        self.tokens.insert(symbol.to_string(), info).is_none()
    }

    /// Remove a token from the registry.
    pub fn remove(&mut self, symbol: &str) -> bool {
        self.tokens.remove(symbol).is_some()
    }

    /// Lookup a token.
    pub fn get(&self, symbol: &str) -> Option<&TokenInfo> {
        self.tokens.get(symbol)
    }

    /// List all token symbols in canonical (sorted) order.
    pub fn list(&self) -> Vec<String> {
        let mut v: Vec<String> = self.tokens.keys().cloned().collect();
        v.sort();
        v
    }
}
