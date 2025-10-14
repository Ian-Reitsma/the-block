use super::peer_metrics_binary;
#[cfg(feature = "telemetry")]
use crate::net::peer::DropReason;
use crate::net::peer::PeerMetrics;
#[cfg(feature = "telemetry")]
use crate::telemetry::{verbose, PEER_RATE_LIMIT_TOTAL};
use concurrency::{MutexExt, OnceCell};
use sled::Tree;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct PeerMetricsStore {
    tree: Tree,
}

impl PeerMetricsStore {
    pub fn open(path: &str) -> sled::Result<Self> {
        let db = sled::open(path)?;
        let tree = db.open_tree("peer_metrics")?;
        Ok(Self { tree })
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn insert(&self, pk: &[u8; 32], metrics: &PeerMetrics, retention: u64) {
        let ts = Self::now();
        let mut key = Vec::with_capacity(40);
        key.extend_from_slice(pk);
        key.extend_from_slice(&ts.to_be_bytes());
        if let Ok(val) = peer_metrics_binary::encode(metrics) {
            let _ = self.tree.insert(key, val);
        }
        #[cfg(feature = "telemetry")]
        if verbose() {
            if let Some(cnt) = metrics.drops.get(&DropReason::RateLimit) {
                let peer_hex = crypto_suite::hex::encode(pk);
                PEER_RATE_LIMIT_TOTAL
                    .ensure_handle_for_label_values(&[peer_hex.as_str()])
                    .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                    .set(*cnt as i64);
            }
        }
        self.prune(pk, retention);
    }

    fn prune(&self, pk: &[u8; 32], retention: u64) {
        let cutoff = Self::now().saturating_sub(retention);
        let keys: Vec<Vec<u8>> = self
            .tree
            .scan_prefix(pk)
            .filter_map(|res| res.ok())
            .filter_map(|(k, _)| {
                if k.len() != 40 {
                    return None;
                }
                let mut ts_bytes = [0u8; 8];
                ts_bytes.copy_from_slice(&k[32..]);
                let ts = u64::from_be_bytes(ts_bytes);
                if ts < cutoff {
                    Some(k.to_vec())
                } else {
                    None
                }
            })
            .collect();
        for k in keys {
            let _ = self.tree.remove(k);
        }
    }

    pub fn load(&self, retention: u64) -> HashMap<[u8; 32], PeerMetrics> {
        let now = Self::now();
        let mut latest: HashMap<[u8; 32], (u64, PeerMetrics)> = HashMap::new();
        let mut stale = Vec::new();
        for res in self.tree.iter().filter_map(|r| r.ok()) {
            let k = res.0;
            if k.len() != 40 {
                continue;
            }
            let mut pk = [0u8; 32];
            pk.copy_from_slice(&k[..32]);
            let mut ts_bytes = [0u8; 8];
            ts_bytes.copy_from_slice(&k[32..]);
            let ts = u64::from_be_bytes(ts_bytes);
            if now.saturating_sub(ts) > retention {
                stale.push(k.to_vec());
                continue;
            }
            if let Ok(m) = peer_metrics_binary::decode(&res.1) {
                match latest.get(&pk) {
                    Some((prev, _)) if *prev >= ts => {}
                    _ => {
                        latest.insert(pk, (ts, m));
                    }
                }
            }
        }
        for k in stale {
            let _ = self.tree.remove(k);
        }
        latest.into_iter().map(|(k, (_ts, m))| (k, m)).collect()
    }

    pub fn flush(&self) -> sled::Result<()> {
        self.tree.flush().map(|_| ())
    }

    pub fn count(&self) -> usize {
        self.tree.len()
    }
}

static STORE: OnceCell<Mutex<Option<PeerMetricsStore>>> = OnceCell::new();

pub fn init(path: &str) {
    let store = PeerMetricsStore::open(path).ok();
    if let Some(cell) = STORE.get() {
        *cell.guard() = store;
    } else {
        let _ = STORE.set(Mutex::new(store));
    }
}

pub fn store() -> Option<PeerMetricsStoreGuard<'static>> {
    STORE
        .get()
        .map(|m| PeerMetricsStoreGuard { inner: m.guard() })
        .and_then(|g| if g.inner.is_some() { Some(g) } else { None })
}

pub struct PeerMetricsStoreGuard<'a> {
    inner: std::sync::MutexGuard<'a, Option<PeerMetricsStore>>,
}

impl<'a> std::ops::Deref for PeerMetricsStoreGuard<'a> {
    type Target = PeerMetricsStore;
    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().unwrap()
    }
}
