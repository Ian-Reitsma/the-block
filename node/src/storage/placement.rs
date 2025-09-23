use super::pipeline::{Provider, LOSS_HI, RTT_HI_MS};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct ProviderEntry {
    provider: Arc<dyn Provider>,
    last_probe: Instant,
    rtt_ewma: f64,
    loss_ewma: f64,
    throughput_ewma: f64,
    success_rate: f64,
    maintenance: bool,
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

#[derive(Clone)]
pub struct NodeHandle {
    pub id: String,
    pub provider: Arc<dyn Provider>,
    pub rtt: f64,
    pub loss: f64,
    pub throughput: f64,
    pub maintenance: bool,
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
                throughput_ewma: 0.0,
                success_rate: 0.0,
                maintenance: false,
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
            .filter(|entry| !entry.maintenance)
            .map(|e| Arc::clone(&e.provider))
            .collect()
    }

    pub fn ranked_nodes(&self) -> Vec<NodeHandle> {
        let mut handles: Vec<NodeHandle> = self
            .providers
            .iter()
            .map(|(id, entry)| NodeHandle {
                id: id.clone(),
                provider: Arc::clone(&entry.provider),
                rtt: entry.rtt_ewma,
                loss: entry.loss_ewma,
                throughput: entry.throughput_ewma,
                maintenance: entry.maintenance,
            })
            .collect();
        handles.sort_by(|a, b| {
            let score_a = (1.0 - a.loss).max(0.05) / a.rtt.max(1.0);
            let score_b = (1.0 - b.loss).max(0.05) / b.rtt.max(1.0);
            score_b.partial_cmp(&score_a).unwrap_or(Ordering::Equal)
        });
        handles
    }

    pub fn stats(&self, id: &str) -> (f64, f64) {
        self.providers
            .get(id)
            .map(|e| (e.rtt_ewma, e.loss_ewma))
            .unwrap_or((0.0, 0.0))
    }

    pub fn record_chunk_result(
        &mut self,
        id: &str,
        bytes: usize,
        duration: Duration,
        success: bool,
    ) {
        if let Some(entry) = self.providers.get_mut(id) {
            let throughput = if duration.as_secs_f64() > 0.0 {
                bytes as f64 / duration.as_secs_f64()
            } else {
                bytes as f64
            };
            entry.throughput_ewma = ewma(entry.throughput_ewma, throughput);
            if success {
                entry.loss_ewma = ewma(entry.loss_ewma, 0.0);
                entry.success_rate = ewma(entry.success_rate, 1.0);
            } else {
                entry.loss_ewma = ewma(entry.loss_ewma, 1.0);
                entry.success_rate = ewma(entry.success_rate, 0.0);
            }
        }
    }

    pub fn mark_maintenance(&mut self, id: &str, maintenance: bool) {
        if let Some(entry) = self.providers.get_mut(id) {
            entry.maintenance = maintenance;
        }
    }

    pub fn maintenance(&self, id: &str) -> bool {
        self.providers
            .get(id)
            .map(|entry| entry.maintenance)
            .unwrap_or(false)
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
            if !entry.maintenance && (entry.loss_ewma > LOSS_HI || entry.rtt_ewma > RTT_HI_MS) {
                drop.push(id.clone());
            }
        }
        for id in drop {
            self.providers.remove(&id);
        }
    }
}

pub fn spawn_probe(catalog: Arc<std::sync::Mutex<NodeCatalog>>, interval: Duration) {
    runtime::spawn(async move {
        let mut tick = runtime::interval(interval);
        loop {
            tick.tick().await;
            if let Ok(mut cat) = catalog.lock() {
                cat.probe_and_prune();
            }
        }
    });
}
