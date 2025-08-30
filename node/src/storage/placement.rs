use super::pipeline::{Provider, LOSS_HI, RTT_HI_MS};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct ProviderEntry {
    provider: Arc<dyn Provider>,
    last_probe: Instant,
    rtt_ewma: f64,
    loss_ewma: f64,
}

fn ewma(prev: f64, new: f64) -> f64 {
    if prev == 0.0 {
        new
    } else {
        prev * 0.8 + new * 0.2
    }
}

#[derive(Default)]
pub struct NodeCatalog {
    providers: HashMap<String, ProviderEntry>,
}

impl NodeCatalog {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register_arc(&mut self, provider: Arc<dyn Provider>) {
        let id = provider.id().to_string();
        self.providers.insert(
            id,
            ProviderEntry {
                provider,
                last_probe: Instant::now(),
                rtt_ewma: 0.0,
                loss_ewma: 0.0,
            },
        );
    }

    pub fn register<P>(&mut self, provider: P)
    where
        P: Provider + Send + Sync + 'static,
    {
        self.register_arc(Arc::new(provider));
    }

    pub fn healthy_nodes(&self) -> Vec<Arc<dyn Provider>> {
        self.providers
            .values()
            .map(|e| Arc::clone(&e.provider))
            .collect()
    }

    pub fn stats(&self, id: &str) -> (f64, f64) {
        self.providers
            .get(id)
            .map(|e| (e.rtt_ewma, e.loss_ewma))
            .unwrap_or((0.0, 0.0))
    }

    pub fn probe_and_prune(&mut self) {
        let mut drop = Vec::new();
        for (id, entry) in self.providers.iter_mut() {
            match entry.provider.probe() {
                Ok(rtt) => {
                    entry.rtt_ewma = ewma(entry.rtt_ewma, rtt);
                    entry.loss_ewma = ewma(entry.loss_ewma, 0.0);
                }
                Err(_) => {
                    entry.loss_ewma = ewma(entry.loss_ewma, 1.0);
                }
            }
            entry.last_probe = Instant::now();
            if entry.loss_ewma > LOSS_HI || entry.rtt_ewma > RTT_HI_MS {
                drop.push(id.clone());
            }
        }
        for id in drop {
            self.providers.remove(&id);
        }
    }
}

pub fn spawn_probe(catalog: Arc<std::sync::Mutex<NodeCatalog>>, interval: Duration) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        loop {
            tick.tick().await;
            if let Ok(mut cat) = catalog.lock() {
                cat.probe_and_prune();
            }
        }
    });
}
