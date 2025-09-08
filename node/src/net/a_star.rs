#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};

/// Cache for ASN latency floor values with simple LRU eviction.
#[derive(Default)]
pub struct AsnLatencyCache {
    map: HashMap<(u32, u32), u64>,
    order: VecDeque<(u32, u32)>,
    cap: usize,
}

impl AsnLatencyCache {
    pub fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap,
        }
    }

    pub fn get_or_insert(&mut self, a: u32, b: u32, compute: impl Fn(u32, u32) -> u64) -> u64 {
        let key = if a <= b { (a, b) } else { (b, a) };
        if let Some(v) = self.map.get(&key) {
            return *v;
        }
        let v = compute(key.0, key.1);
        if self.order.len() == self.cap {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        self.order.push_back(key);
        self.map.insert(key, v);
        v
    }

    /// Recompute cached latency floors using the provided measurement function.
    pub fn recompute(&mut self, measure: impl Fn(u32, u32) -> u64) {
        let keys: Vec<(u32, u32)> = self.map.keys().copied().collect();
        for k in keys {
            let v = measure(k.0, k.1);
            self.map.insert(k, v);
        }
    }
}

/// Admissible heuristic combining ASN latency floor and uptime penalty.
pub fn heuristic(
    cache: &mut AsnLatencyCache,
    asn_src: u32,
    asn_dst: u32,
    uptime: f64,
    mu: f64,
) -> f64 {
    let floor = cache.get_or_insert(asn_src, asn_dst, |_a, _b| 0);
    floor as f64 + mu * (1.0 - uptime)
}
