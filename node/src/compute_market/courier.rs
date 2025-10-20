use super::scheduler::{self, Capability};
use crate::util::binary_struct::{assign_once, decode_struct, ensure_exhausted, DecodeError};
use concurrency::{mutex, Lazy, MutexExt, MutexGuard, MutexT};
use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::binary_cursor::{Reader, Writer};
use rand::RngCore;
use runtime::{block_on, sleep};
use sled::Tree;
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Receipt stored for carry-to-earn courier mode.
#[derive(Clone, Debug, PartialEq)]
pub struct CourierReceipt {
    pub id: u64,
    pub bundle_hash: String,
    pub sender: String,
    pub timestamp: u64,
    pub acknowledged: bool,
}

impl CourierReceipt {
    fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::with_capacity(128);
        writer.write_struct(|s| {
            s.field_u64("id", self.id);
            s.field_string("bundle_hash", &self.bundle_hash);
            s.field_string("sender", &self.sender);
            s.field_u64("timestamp", self.timestamp);
            s.field_bool("acknowledged", self.acknowledged);
        });
        writer.finish()
    }

    fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        let mut id = None;
        let mut bundle_hash = None;
        let mut sender = None;
        let mut timestamp = None;
        let mut acknowledged = None;

        decode_struct(&mut reader, Some(5), |key, reader| match key {
            "id" => {
                let value = reader.read_u64()?;
                assign_once(&mut id, value, "id")
            }
            "bundle_hash" => {
                let value = reader.read_string()?;
                assign_once(&mut bundle_hash, value, "bundle_hash")
            }
            "sender" => {
                let value = reader.read_string()?;
                assign_once(&mut sender, value, "sender")
            }
            "timestamp" => {
                let value = reader.read_u64()?;
                assign_once(&mut timestamp, value, "timestamp")
            }
            "acknowledged" => {
                let value = reader.read_bool()?;
                assign_once(&mut acknowledged, value, "acknowledged")
            }
            other => Err(DecodeError::UnknownField(other.to_owned())),
        })?;

        ensure_exhausted(&reader)?;

        Ok(CourierReceipt {
            id: id.ok_or(DecodeError::MissingField("id"))?,
            bundle_hash: bundle_hash.ok_or(DecodeError::MissingField("bundle_hash"))?,
            sender: sender.ok_or(DecodeError::MissingField("sender"))?,
            timestamp: timestamp.ok_or(DecodeError::MissingField("timestamp"))?,
            acknowledged: acknowledged.ok_or(DecodeError::MissingField("acknowledged"))?,
        })
    }
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
        let mut rng = rand::rngs::OsRng::default();
        let id = rng.next_u64();
        let receipt = CourierReceipt {
            id,
            bundle_hash,
            sender: sender.to_string(),
            timestamp: ts,
            acknowledged: false,
        };
        let bytes = receipt.encode();
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
                if let Ok(mut rec) = CourierReceipt::decode(&v) {
                    if rec.acknowledged {
                        continue;
                    }
                    let mut attempt = 0u32;
                    let mut delay = Duration::from_millis(100);
                    loop {
                        #[cfg(feature = "telemetry")]
                        crate::telemetry::COURIER_FLUSH_ATTEMPT_TOTAL.inc();
                        #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                        diagnostics::tracing::info!(id = rec.id, sender = %rec.sender, attempt, "courier flush attempt");
                        if forward(&rec) {
                            rec.acknowledged = true;
                            let bytes = rec.encode();
                            if let Err(e) = self.tree.insert(&k, bytes) {
                                #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                                diagnostics::tracing::error!("courier update failed: {e}");
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
                            diagnostics::tracing::warn!(
                                id = rec.id,
                                attempt,
                                "courier forward failed"
                            );
                            attempt += 1;
                            if attempt >= 5 {
                                break;
                            }
                            let wait = take_backoff_delay(&mut delay);
                            block_on(sleep(wait));
                        }
                    }
                }
            }
        }
        Ok(acknowledged)
    }

    pub async fn flush_async<F, Fut>(&self, forward: F) -> Result<u64, sled::Error>
    where
        F: Fn(&CourierReceipt) -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let mut acknowledged = 0u64;
        let keys: Vec<_> = self
            .tree
            .iter()
            .map(|res| res.map(|(k, _v)| k))
            .collect::<Result<Vec<_>, _>>()?;
        for k in keys {
            if let Some(v) = self.tree.get(&k)? {
                if let Ok(mut rec) = CourierReceipt::decode(&v) {
                    if rec.acknowledged {
                        continue;
                    }
                    let mut attempt = 0u32;
                    let mut delay = Duration::from_millis(100);
                    loop {
                        #[cfg(feature = "telemetry")]
                        crate::telemetry::COURIER_FLUSH_ATTEMPT_TOTAL.inc();
                        #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                        diagnostics::tracing::info!(id = rec.id, sender = %rec.sender, attempt, "courier flush attempt");
                        if forward(&rec).await {
                            rec.acknowledged = true;
                            let bytes = rec.encode();
                            if let Err(e) = self.tree.insert(&k, bytes) {
                                #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                                diagnostics::tracing::error!("courier update failed: {e}");
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
                            diagnostics::tracing::warn!(
                                id = rec.id,
                                attempt,
                                "courier forward failed"
                            );
                            attempt += 1;
                            if attempt >= 5 {
                                break;
                            }
                            let wait = take_backoff_delay(&mut delay);
                            sleep(wait).await;
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
            .and_then(|v| CourierReceipt::decode(&v).ok())
    }
}

fn take_backoff_delay(delay: &mut Duration) -> Duration {
    let current = *delay;
    *delay = delay.saturating_mul(2);
    current
}

use std::sync::atomic::{AtomicBool, Ordering};

static HANDOFF_FAIL: AtomicBool = AtomicBool::new(false);
static CANCELED: Lazy<MutexT<HashSet<String>>> = Lazy::new(|| mutex(HashSet::new()));
static HALTED: Lazy<MutexT<HashSet<String>>> = Lazy::new(|| mutex(HashSet::new()));
static RESERVED: Lazy<MutexT<HashSet<String>>> = Lazy::new(|| mutex(HashSet::new()));

fn canceled_jobs() -> MutexGuard<'static, HashSet<String>> {
    CANCELED.guard()
}

fn halted_jobs() -> MutexGuard<'static, HashSet<String>> {
    HALTED.guard()
}

fn reserved_jobs() -> MutexGuard<'static, HashSet<String>> {
    RESERVED.guard()
}

pub fn handoff_job(job_id: &str, new_provider: &str) -> Result<(), &'static str> {
    if HANDOFF_FAIL.load(Ordering::Relaxed) {
        return Err("handoff failed");
    }
    if canceled_jobs().remove(job_id) {
        return Err("job cancelled");
    }
    #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
    diagnostics::tracing::info!(job_id, provider = new_provider, "courier handoff");
    #[cfg(not(any(feature = "telemetry", feature = "test-telemetry")))]
    let _ = new_provider;
    Ok(())
}

pub fn set_handoff_fail(val: bool) {
    HANDOFF_FAIL.store(val, Ordering::Relaxed);
}

pub fn cancel_job(job_id: &str) {
    canceled_jobs().insert(job_id.to_owned());
}

pub fn halt_job(job_id: &str) {
    halted_jobs().insert(job_id.to_owned());
}

pub fn was_halted(job_id: &str) -> bool {
    halted_jobs().contains(job_id)
}

pub fn reserve_resources(job_id: &str) {
    reserved_jobs().insert(job_id.to_owned());
}

pub fn release_resources(job_id: &str) -> bool {
    reserved_jobs().remove(job_id)
}

pub fn is_reserved(job_id: &str) -> bool {
    reserved_jobs().contains(job_id)
}
