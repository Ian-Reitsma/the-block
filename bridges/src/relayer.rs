use std::collections::HashMap;

#[cfg(feature = "telemetry")]
use crate::{telemetry_counter, BRIDGE_SLASHES_TOTAL};
#[cfg(feature = "telemetry")]
use concurrency::Lazy;
#[cfg(feature = "telemetry")]
use runtime::telemetry::Counter;

#[cfg(feature = "telemetry")]
fn relayer_slash_counter() -> Counter {
    telemetry_counter(
        "relayer_slash_total",
        "Total slashing events for bridge relayers",
    )
}

#[cfg(feature = "telemetry")]
pub static RELAYER_SLASH_TOTAL: Lazy<Counter> = Lazy::new(relayer_slash_counter);

#[derive(Debug, Clone, Default)]
pub struct Relayer {
    pub stake: u64,
    pub slashes: u64,
}

#[derive(Debug, Clone, Default)]
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

    pub(crate) fn insert_state(&mut self, id: String, relayer: Relayer) {
        self.relayers.insert(id, relayer);
    }
}
