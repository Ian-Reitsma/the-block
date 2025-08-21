use blake3::Hasher;
use serde::{Deserialize, Serialize};
use sled::Tree;
use std::time::{SystemTime, UNIX_EPOCH};

/// Receipt stored for carry-to-earn courier mode.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CourierReceipt {
    pub bundle_hash: String,
    pub sender: String,
    pub timestamp: u64,
    pub delivered: bool,
}

pub struct CourierStore {
    tree: Tree,
}

impl CourierStore {
    pub fn open(path: &str) -> Self {
        let db = sled::open(path).unwrap();
        let tree = db.open_tree("courier").unwrap();
        Self { tree }
    }

    pub fn send(&self, bundle: &[u8], sender: &str) -> CourierReceipt {
        let mut h = Hasher::new();
        h.update(bundle);
        let bundle_hash = h.finalize().to_hex().to_string();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let receipt = CourierReceipt {
            bundle_hash: bundle_hash.clone(),
            sender: sender.to_string(),
            timestamp: ts,
            delivered: false,
        };
        let bytes = bincode::serialize(&receipt).unwrap();
        let _ = self.tree.insert(bundle_hash.as_bytes(), bytes);
        receipt
    }

    pub fn flush<F: Fn(&CourierReceipt) -> bool>(&self, forward: F) -> Result<u64, sled::Error> {
        let mut forwarded = 0u64;
        let mut iter = self.tree.iter();
        while let Some(next) = iter.next() {
            let (k, v) = match next {
                Ok(kv) => kv,
                Err(e) => {
                    #[cfg(feature = "telemetry")]
                    log::error!("courier scan failed: {e}");
                    #[cfg(not(feature = "telemetry"))]
                    eprintln!("courier scan failed: {e}");
                    return Err(e);
                }
            };
            if let Ok(rec) = bincode::deserialize::<CourierReceipt>(&v) {
                if forward(&rec) {
                    if let Err(e) = self.tree.remove(&k) {
                        #[cfg(feature = "telemetry")]
                        log::error!("courier remove failed: {e}");
                        #[cfg(not(feature = "telemetry"))]
                        eprintln!("courier remove failed: {e}");
                        return Err(e);
                    }
                    forwarded += 1;
                }
            }
        }
        Ok(forwarded)
    }
}
