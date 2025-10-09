use crate::compute_market::receipt::Receipt;
use crate::transaction::FeeLane;
use foundation_serialization::binary;
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
                    if let Ok(r) = binary::decode::<Receipt>(&v) {
                        seen.insert(r.idempotency_key);
                    } else {
                        #[cfg(feature = "telemetry")]
                        crate::telemetry::RECEIPT_CORRUPT_TOTAL.inc();
                        #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                        diagnostics::tracing::warn!("corrupt receipt for key {:?}", key);
                        #[cfg(all(not(feature = "telemetry"), not(feature = "test-telemetry")))]
                        let _ = key;
                    }
                }
                Err(err) => {
                    #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                    diagnostics::tracing::error!("iterate receipts: {err}");
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
        let bytes = binary::encode(r).unwrap_or_else(|e| panic!("serialize receipt: {e}"));
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

    pub fn recent_by_lane(&self, lane: FeeLane, limit: usize) -> Result<Vec<Receipt>, sled::Error> {
        let mut receipts = Vec::new();
        for entry in self.tree.iter() {
            match entry {
                Ok((_key, bytes)) => {
                    if let Ok(receipt) = binary::decode::<Receipt>(&bytes) {
                        if receipt.lane == lane {
                            receipts.push(receipt);
                        }
                    }
                }
                Err(err) => return Err(err),
            }
        }
        receipts.sort_by(|a, b| b.issued_at.cmp(&a.issued_at));
        if limit > 0 && receipts.len() > limit {
            receipts.truncate(limit);
        }
        Ok(receipts)
    }
}
