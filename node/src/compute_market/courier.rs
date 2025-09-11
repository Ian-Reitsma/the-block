use super::scheduler::{self, Capability};
use blake3::Hasher;
use once_cell::sync::Lazy;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sled::Tree;
use std::collections::HashSet;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Receipt stored for carry-to-earn courier mode.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CourierReceipt {
    pub id: u64,
    pub bundle_hash: String,
    pub sender: String,
    pub timestamp: u64,
    pub acknowledged: bool,
}

pub struct CourierStore {
    tree: Tree,
}

impl CourierStore {
    pub fn open(path: &str) -> Self {
        let db = sled::open(path).unwrap_or_else(|e| panic!("open courier db: {e}"));
        let tree = db
            .open_tree("courier")
            .unwrap_or_else(|e| panic!("open courier tree: {e}"));
        Self { tree }
    }

    pub fn send(&self, bundle: &[u8], sender: &str) -> CourierReceipt {
        let mut h = Hasher::new();
        h.update(bundle);
        let bundle_hash = h.finalize().to_hex().to_string();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| panic!("time error: {e}"))
            .as_secs();
        let id = rand::rngs::OsRng.next_u64();
        let receipt = CourierReceipt {
            id,
            bundle_hash,
            sender: sender.to_string(),
            timestamp: ts,
            acknowledged: false,
        };
        let bytes =
            bincode::serialize(&receipt).unwrap_or_else(|e| panic!("serialize receipt: {e}"));
        let _ = self.tree.insert(id.to_be_bytes(), bytes);
        receipt
    }

    /// Send a bundle only if a provider matching the required capability exists.
    /// Returns `None` when no compatible provider is available.
    pub fn send_for_capability(
        &self,
        bundle: &[u8],
        sender: &str,
        need: &Capability,
    ) -> Option<CourierReceipt> {
        scheduler::match_offer(need).map(|_| self.send(bundle, sender))
    }

    pub fn flush<F: Fn(&CourierReceipt) -> bool>(&self, forward: F) -> Result<u64, sled::Error> {
        let mut acknowledged = 0u64;
        let keys: Vec<_> = self
            .tree
            .iter()
            .map(|res| res.map(|(k, _v)| k))
            .collect::<Result<Vec<_>, _>>()?;
        for k in keys {
            if let Some(v) = self.tree.get(&k)? {
                if let Ok(mut rec) = bincode::deserialize::<CourierReceipt>(&v) {
                    if rec.acknowledged {
                        continue;
                    }
                    let mut attempt = 0u32;
                    let mut delay = Duration::from_millis(100);
                    loop {
                        #[cfg(feature = "telemetry")]
                        crate::telemetry::COURIER_FLUSH_ATTEMPT_TOTAL.inc();
                        #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                        tracing::info!(id = rec.id, sender = %rec.sender, attempt, "courier flush attempt");
                        if forward(&rec) {
                            rec.acknowledged = true;
                            let bytes = bincode::serialize(&rec)
                                .unwrap_or_else(|e| panic!("serialize receipt: {e}"));
                            if let Err(e) = self.tree.insert(&k, bytes) {
                                #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                                tracing::error!("courier update failed: {e}");
                                #[cfg(all(
                                    not(feature = "telemetry"),
                                    not(feature = "test-telemetry")
                                ))]
                                eprintln!("courier update failed: {e}");
                                return Err(e);
                            }
                            acknowledged += 1;
                            break;
                        } else {
                            #[cfg(feature = "telemetry")]
                            crate::telemetry::COURIER_FLUSH_FAILURE_TOTAL.inc();
                            #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                            tracing::warn!(id = rec.id, attempt, "courier forward failed");
                            attempt += 1;
                            if attempt >= 5 {
                                break;
                            }
                            thread::sleep(delay);
                            delay *= 2;
                        }
                    }
                }
            }
        }
        Ok(acknowledged)
    }

    pub fn get(&self, id: u64) -> Option<CourierReceipt> {
        self.tree
            .get(id.to_be_bytes())
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize(&v).ok())
    }
}

use std::sync::atomic::{AtomicBool, Ordering};

static HANDOFF_FAIL: AtomicBool = AtomicBool::new(false);
static CANCELED: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static HALTED: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static RESERVED: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

pub fn handoff_job(job_id: &str, new_provider: &str) -> Result<(), &'static str> {
    if HANDOFF_FAIL.load(Ordering::Relaxed) {
        return Err("handoff failed");
    }
    if CANCELED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(job_id)
    {
        return Err("job cancelled");
    }
    #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
    tracing::info!(job_id, provider = new_provider, "courier handoff");
    #[cfg(not(any(feature = "telemetry", feature = "test-telemetry")))]
    let _ = new_provider;
    Ok(())
}

pub fn set_handoff_fail(val: bool) {
    HANDOFF_FAIL.store(val, Ordering::Relaxed);
}

pub fn cancel_job(job_id: &str) {
    CANCELED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(job_id.to_owned());
}

pub fn halt_job(job_id: &str) {
    HALTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(job_id.to_owned());
}

pub fn was_halted(job_id: &str) -> bool {
    HALTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(job_id)
}

pub fn reserve_resources(job_id: &str) {
    RESERVED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(job_id.to_owned());
}

pub fn release_resources(job_id: &str) -> bool {
    RESERVED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(job_id)
}

pub fn is_reserved(job_id: &str) -> bool {
    RESERVED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(job_id)
}
