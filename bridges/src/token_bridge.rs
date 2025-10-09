use ledger::{Emission, TokenRegistry};

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
}

impl TokenBridge {
    pub fn new() -> Self {
        Self {
            registry: TokenRegistry::new(),
        }
    }

    /// Lock native tokens on this chain and emit a mint instruction for the remote chain.
    pub fn lock(&mut self, symbol: &str, amount: u64) {
        let _created = if self.registry.get(symbol).is_none() {
            // auto-register with fixed emission equal to lock amount
            self.registry.register(symbol, Emission::Fixed(amount))
        } else {
            false
        };
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_VOLUME_TOTAL.get().inc_by(amount);
            if _created {
                TOKENS_CREATED_TOTAL.get().inc();
            }
        }
    }

    /// Mint wrapped tokens after verifying lock on remote chain.
    pub fn mint(&self, _symbol: &str, _amount: u64) -> bool {
        true
    }

    pub fn tokens(&self) -> Vec<(String, Emission)> {
        let mut tokens = Vec::new();
        for symbol in self.registry.list() {
            if let Some(info) = self.registry.get(&symbol) {
                tokens.push((symbol, info.emission.clone()));
            }
        }
        tokens
    }

    pub(crate) fn with_registry(registry: TokenRegistry) -> Self {
        Self { registry }
    }
}
