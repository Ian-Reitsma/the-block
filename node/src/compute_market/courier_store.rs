use crate::compute_market::receipt::Receipt;
use sled::Tree;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ReceiptStore {
    tree: Tree,
    seen: Arc<Mutex<HashSet<[u8; 32]>>>,
}

impl ReceiptStore {
    pub fn open(path: &str) -> Self {
        let db = sled::open(path).unwrap_or_else(|e| panic!("open receipt db: {e}"));
        let tree = db
            .open_tree("receipts")
            .unwrap_or_else(|e| panic!("open receipt tree: {e}"));
        let mut seen = HashSet::new();
        for entry in tree.iter() {
            match entry {
                Ok((key, v)) => {
                    if let Ok(r) = bincode::deserialize::<Receipt>(&v) {
                        seen.insert(r.idempotency_key);
                    } else {
                        #[cfg(feature = "telemetry")]
                        crate::telemetry::RECEIPT_CORRUPT_TOTAL.inc();
                        #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                        tracing::warn!("corrupt receipt for key {:?}", key);
                        #[cfg(all(not(feature = "telemetry"), not(feature = "test-telemetry")))]
                        let _ = key;
                    }
                }
                Err(err) => {
                    #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                    tracing::error!("iterate receipts: {err}");
                    #[cfg(all(not(feature = "telemetry"), not(feature = "test-telemetry")))]
                    let _ = err;
                }
            }
        }
        Self {
            tree,
            seen: Arc::new(Mutex::new(seen)),
        }
    }

    /// Attempt to insert the receipt; returns `true` if newly stored.
    pub fn try_insert(&self, r: &Receipt) -> Result<bool, sled::Error> {
        let key = r.idempotency_key;
        let bytes = bincode::serialize(r).unwrap_or_else(|e| panic!("serialize receipt: {e}"));
        let res = self
            .tree
            .compare_and_swap(key, None as Option<Vec<u8>>, Some(bytes))?;
        if res.is_ok() {
            self.seen
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn len(&self) -> Result<usize, sled::Error> {
        Ok(self.tree.len() as usize)
    }
}
