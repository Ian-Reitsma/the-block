use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "telemetry")]
use crate::BRIDGE_SLASHES_TOTAL;
#[cfg(feature = "telemetry")]
use once_cell::sync::Lazy;
#[cfg(feature = "telemetry")]
use prometheus::{IntCounter, Opts, Registry};

#[cfg(feature = "telemetry")]
static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

#[cfg(feature = "telemetry")]
pub static RELAYER_SLASH_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "relayer_slash_total",
        "Total slashing events for bridge relayers",
    ))
    .expect("counter");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
});

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Relayer {
    pub stake: u64,
    pub slashes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RelayerSet {
    relayers: HashMap<String, Relayer>,
}

impl RelayerSet {
    pub fn stake(&mut self, id: &str, amount: u64) {
        let entry = self.relayers.entry(id.to_string()).or_default();
        entry.stake += amount;
    }

    pub fn status(&self, id: &str) -> Option<&Relayer> {
        self.relayers.get(id)
    }

    pub fn slash(&mut self, id: &str, amount: u64) {
        if let Some(r) = self.relayers.get_mut(id) {
            if r.stake >= amount {
                r.stake -= amount;
            } else {
                r.stake = 0;
            }
            r.slashes += 1;
            #[cfg(feature = "telemetry")]
            {
                RELAYER_SLASH_TOTAL.inc();
                BRIDGE_SLASHES_TOTAL.inc();
            }
        }
    }

    pub fn snapshot(&self) -> HashMap<String, Relayer> {
        self.relayers.clone()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Relayer)> {
        self.relayers.iter()
    }
}
