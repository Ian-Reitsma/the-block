use ledger::{Emission, TokenRegistry};
use std::collections::{BTreeSet, HashMap};

#[cfg(feature = "telemetry")]
use crate::telemetry_counter;
#[cfg(feature = "telemetry")]
use concurrency::Lazy;
#[cfg(feature = "telemetry")]
use runtime::telemetry::Counter;

#[cfg(feature = "telemetry")]
fn bridge_volume_counter() -> Counter {
    telemetry_counter(
        "token_bridge_volume_total",
        "Total volume bridged via token bridge",
    )
}

#[cfg(feature = "telemetry")]
static BRIDGE_VOLUME_TOTAL: Lazy<Counter> = Lazy::new(bridge_volume_counter);

#[cfg(feature = "telemetry")]
fn tokens_created_counter() -> Counter {
    telemetry_counter("tokens_created_total", "Total number of tokens registered")
}

#[cfg(feature = "telemetry")]
static TOKENS_CREATED_TOTAL: Lazy<Counter> = Lazy::new(tokens_created_counter);

/// Simplified token bridge that locks tokens and mints wrapped assets.
#[derive(Debug, Clone, Default)]
pub struct TokenBridge {
    registry: TokenRegistry,
    locked_supply: HashMap<String, u64>,
    minted_supply: HashMap<String, u64>,
}

#[derive(Debug, Clone)]
pub struct AssetSnapshot {
    pub symbol: String,
    pub emission: Emission,
    pub locked: u64,
    pub minted: u64,
}

impl TokenBridge {
    pub fn new() -> Self {
        Self {
            registry: TokenRegistry::new(),
            locked_supply: HashMap::new(),
            minted_supply: HashMap::new(),
        }
    }

    /// Lock native tokens on this chain and emit a mint instruction for the remote chain.
    pub fn lock(&mut self, symbol: &str, amount: u64) {
        let created = self.ensure_token_registered(symbol);
        #[cfg(not(feature = "telemetry"))]
        let _ = created;
        let entry = self.locked_supply.entry(symbol.to_string()).or_insert(0);
        *entry = entry.saturating_add(amount);
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_VOLUME_TOTAL.get().inc_by(amount);
            if created {
                TOKENS_CREATED_TOTAL.get().inc();
            }
        }
    }

    /// Mint wrapped tokens after verifying lock on remote chain.
    pub fn mint(&mut self, symbol: &str, amount: u64) -> bool {
        self.ensure_token_registered(symbol);
        let entry = self.minted_supply.entry(symbol.to_string()).or_insert(0);
        *entry = entry.saturating_add(amount);
        true
    }

    /// Reduce the locked supply after tokens leave the bridge.
    pub fn unlock(&mut self, symbol: &str, amount: u64) {
        if amount == 0 {
            return;
        }
        if let Some(entry) = self.locked_supply.get_mut(symbol) {
            *entry = entry.saturating_sub(amount);
            if *entry == 0 {
                self.locked_supply.remove(symbol);
            }
        }
    }

    /// Burn wrapped tokens after they return through the bridge.
    pub fn burn(&mut self, symbol: &str, amount: u64) {
        if amount == 0 {
            return;
        }
        if let Some(entry) = self.minted_supply.get_mut(symbol) {
            *entry = entry.saturating_sub(amount);
            if *entry == 0 {
                self.minted_supply.remove(symbol);
            }
        }
    }

    pub fn tokens(&self) -> Vec<(String, Emission)> {
        self.registry
            .list()
            .into_iter()
            .filter_map(|symbol| {
                self.registry
                    .get(&symbol)
                    .map(|info| (symbol, info.emission.clone()))
            })
            .collect()
    }

    pub(crate) fn with_state(
        registry: TokenRegistry,
        locked_supply: HashMap<String, u64>,
        minted_supply: HashMap<String, u64>,
    ) -> Self {
        Self {
            registry,
            locked_supply,
            minted_supply,
        }
    }

    pub fn asset_symbols(&self) -> Vec<String> {
        let mut symbols: BTreeSet<String> = self.registry.list().into_iter().collect();
        for key in self.locked_supply.keys().chain(self.minted_supply.keys()) {
            symbols.insert(key.clone());
        }
        symbols.into_iter().collect()
    }

    pub fn asset_snapshots(&self) -> Vec<AssetSnapshot> {
        self.asset_symbols()
            .into_iter()
            .map(|symbol| {
                let emission = self
                    .registry
                    .get(&symbol)
                    .map(|info| info.emission.clone())
                    .unwrap_or_else(|| Emission::Fixed(0));
                AssetSnapshot {
                    locked: self.locked_supply(&symbol),
                    minted: self.minted_supply(&symbol),
                    symbol,
                    emission,
                }
            })
            .collect()
    }

    pub fn locked_supply(&self, symbol: &str) -> u64 {
        self.locked_supply.get(symbol).copied().unwrap_or(0)
    }

    pub fn minted_supply(&self, symbol: &str) -> u64 {
        self.minted_supply.get(symbol).copied().unwrap_or(0)
    }

    fn ensure_token_registered(&mut self, symbol: &str) -> bool {
        if self.registry.get(symbol).is_none() {
            self.registry.register(symbol, Emission::Fixed(0))
        } else {
            false
        }
    }
}
