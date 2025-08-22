use blake3::Hasher;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sled::Tree;
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

    pub fn flush<F: Fn(&CourierReceipt) -> bool>(&self, forward: F) -> Result<u64, sled::Error> {
        let mut acknowledged = 0u64;
        let mut iter = self.tree.iter();
        while let Some(next) = iter.next() {
            let (k, v) = match next {
                Ok(kv) => kv,
                Err(e) => {
                    #[cfg(feature = "telemetry")]
                    tracing::error!("courier scan failed: {e}");
                    #[cfg(not(feature = "telemetry"))]
                    eprintln!("courier scan failed: {e}");
                    return Err(e);
                }
            };
            if let Ok(mut rec) = bincode::deserialize::<CourierReceipt>(&v) {
                if rec.acknowledged {
                    continue;
                }
                let mut attempt = 0u32;
                let mut delay = Duration::from_millis(100);
                loop {
                    #[cfg(feature = "telemetry")]
                    {
                        crate::telemetry::COURIER_FLUSH_ATTEMPT_TOTAL.inc();
                        tracing::info!(id = rec.id, sender = %rec.sender, attempt, "courier flush attempt");
                    }
                    if forward(&rec) {
                        rec.acknowledged = true;
                        let bytes = bincode::serialize(&rec)
                            .unwrap_or_else(|e| panic!("serialize receipt: {e}"));
                        if let Err(e) = self.tree.insert(&k, bytes) {
                            #[cfg(feature = "telemetry")]
                            tracing::error!("courier update failed: {e}");
                            #[cfg(not(feature = "telemetry"))]
                            eprintln!("courier update failed: {e}");
                            return Err(e);
                        }
                        acknowledged += 1;
                        break;
                    } else {
                        #[cfg(feature = "telemetry")]
                        {
                            crate::telemetry::COURIER_FLUSH_FAILURE_TOTAL.inc();
                            tracing::warn!(id = rec.id, attempt, "courier forward failed");
                        }
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
