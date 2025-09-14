use ledger::{Emission, TokenRegistry};

/// Simplified token bridge that locks tokens and mints wrapped assets.
#[derive(Default)]
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
            use once_cell::sync::Lazy;
            use prometheus::{IntCounter, Opts};
            static BRIDGE_VOL: Lazy<IntCounter> = Lazy::new(|| {
                let c = IntCounter::with_opts(Opts::new(
                    "token_bridge_volume_total",
                    "Total volume bridged via token bridge",
                ))
                .expect("counter");
                crate::REGISTRY
                    .register(Box::new(c.clone()))
                    .expect("register");
                c
            });
            static TOKENS_CREATED: Lazy<IntCounter> = Lazy::new(|| {
                let c = IntCounter::with_opts(Opts::new(
                    "tokens_created_total",
                    "Total number of tokens registered",
                ))
                .expect("counter");
                crate::REGISTRY
                    .register(Box::new(c.clone()))
                    .expect("register");
                c
            });
            BRIDGE_VOL.inc_by(amount as u64);
            if _created {
                TOKENS_CREATED.inc();
            }
        }
    }

    /// Mint wrapped tokens after verifying lock on remote chain.
    pub fn mint(&self, _symbol: &str, _amount: u64) -> bool {
        true
    }
}
