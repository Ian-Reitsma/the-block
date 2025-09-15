use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

#[cfg(feature = "telemetry")]
use crate::telemetry::{MOBILE_CACHE_HIT_TOTAL, MOBILE_TX_QUEUE_DEPTH};

/// Simple TTL cache for mobile RPC responses with offline tx queue.
pub struct MobileCache {
    ttl: Duration,
    store: HashMap<String, (Instant, String)>,
    queue: VecDeque<String>,
}

impl MobileCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            store: HashMap::new(),
            queue: VecDeque::new(),
        }
    }

    /// Fetch a cached value if fresh.
    pub fn get(&mut self, key: &str) -> Option<String> {
        if let Some((ts, val)) = self.store.get(key) {
            if ts.elapsed() < self.ttl {
                #[cfg(feature = "telemetry")]
                MOBILE_CACHE_HIT_TOTAL.inc();
                return Some(val.clone());
            }
        }
        None
    }

    /// Insert a value into the cache.
    pub fn insert(&mut self, key: String, val: String) {
        self.store.insert(key, (Instant::now(), val));
    }

    /// Queue a transaction for later submission.
    pub fn queue_tx(&mut self, tx: String) {
        self.queue.push_back(tx);
        #[cfg(feature = "telemetry")]
        MOBILE_TX_QUEUE_DEPTH.set(self.queue.len() as i64);
    }

    /// Drain queued transactions using the provided sender.
    pub fn drain_queue<F>(&mut self, mut send: F)
    where
        F: FnMut(&str),
    {
        while let Some(tx) = self.queue.pop_front() {
            send(&tx);
        }
        #[cfg(feature = "telemetry")]
        MOBILE_TX_QUEUE_DEPTH.set(self.queue.len() as i64);
    }
}
