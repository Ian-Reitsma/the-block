use super::erasure;
use super::types::{ObjectManifest, Redundancy};
use crate::simple_db::SimpleDb;
#[cfg(feature = "telemetry")]
use crate::telemetry::{STORAGE_REPAIR_BYTES_TOTAL, STORAGE_REPAIR_FAILURES_TOTAL};
use raptorq::{Decoder, Encoder, EncodingPacket, ObjectTransmissionInformation};
use std::thread;
use std::time::Duration;
use tokio::task;

/// RaptorQ overlay constants tuned for BLE repair
const SYMBOL_SIZE: u16 = 1024; // 1 KiB symbols
const RATE: f32 = 1.2; // 20% overhead

pub fn spawn(path: String, period: Duration) {
    task::spawn_blocking(move || {
        let mut db = SimpleDb::open(&path);
        loop {
            if let Err(_) = run_once(&mut db) {
                #[cfg(feature = "telemetry")]
                STORAGE_REPAIR_FAILURES_TOTAL.inc();
            }
            notify_iteration();
            if should_stop() {
                break;
            }
            thread::sleep(period);
        }
    });
}

pub fn run_once(db: &mut SimpleDb) -> Result<(), String> {
    let keys = db.keys_with_prefix("manifest/");
    for key in keys {
        let bytes = db.get(&key).ok_or("missing manifest")?;
        let manifest: ObjectManifest = bincode::deserialize(&bytes).map_err(|e| e.to_string())?;
        if let Redundancy::ReedSolomon { data: d, parity: p } = manifest.redundancy {
            let step = (d + p) as usize;
            for group in manifest.chunks.chunks(step) {
                let mut shards = Vec::new();
                let mut missing_idx = None;
                for (i, ch) in group.iter().enumerate() {
                    let blob = db.get(&format!("chunk/{}", hex::encode(ch.id)));
                    if blob.is_none() {
                        missing_idx = Some(i);
                    }
                    shards.push(blob);
                }
                if let Some(idx) = missing_idx {
                    let rebuilt = erasure::reconstruct(shards)?;
                    let key = format!("chunk/{}", hex::encode(group[idx].id));
                    db.insert(&key, rebuilt.clone());
                    #[cfg(feature = "telemetry")]
                    STORAGE_REPAIR_BYTES_TOTAL.inc_by(rebuilt.len() as u64);
                }
            }
        }
    }
    Ok(())
}

/// Encodes `data` into RaptorQ packets with the BLE-tuned parameters and decodes
/// them after dropping a single packet, returning the recovered bytes.
pub fn raptorq_repair_roundtrip(data: &[u8]) -> Result<Vec<u8>, String> {
    let oti = ObjectTransmissionInformation::with_defaults(data.len() as u64, SYMBOL_SIZE);
    let encoder = Encoder::new(data, oti.clone());
    let symbols = (data.len() as f32 / SYMBOL_SIZE as f32).ceil();
    let repair = ((symbols * (RATE - 1.0)).ceil()) as u32;
    let total = symbols as u32;
    let mut packets: Vec<EncodingPacket> = encoder.get_encoded_packets(total + repair);
    if !packets.is_empty() {
        packets.remove(0);
    }
    let mut decoder = Decoder::new(oti);
    for p in packets {
        // feed all packets; decoder yields `None` until enough symbols arrive
        decoder.decode(p);
    }
    decoder.get_result().ok_or_else(|| "decode".to_string())
}

#[cfg(test)]
use once_cell::sync::Lazy;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::sync::Mutex;
#[cfg(test)]
use tokio::sync::mpsc::UnboundedSender;

#[cfg(test)]
static ITERATION_HOOK: Lazy<Mutex<Option<UnboundedSender<()>>>> = Lazy::new(|| Mutex::new(None));
#[cfg(test)]
static STOP_FLAG: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

#[cfg(test)]
pub(crate) fn install_iteration_hook(sender: UnboundedSender<()>) {
    *ITERATION_HOOK.lock().unwrap() = Some(sender);
}

#[cfg(test)]
pub(crate) fn clear_iteration_hook() {
    ITERATION_HOOK.lock().unwrap().take();
    STOP_FLAG.store(false, Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn request_stop() {
    STOP_FLAG.store(true, Ordering::SeqCst);
}

#[cfg(test)]
fn notify_iteration() {
    if let Some(tx) = ITERATION_HOOK.lock().unwrap().as_ref() {
        let _ = tx.send(());
    }
}

#[cfg(not(test))]
fn notify_iteration() {}

#[cfg(test)]
fn should_stop() -> bool {
    STOP_FLAG.load(Ordering::SeqCst)
}

#[cfg(not(test))]
fn should_stop() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::runtime::Builder;

    #[test]
    fn spawn_runs_loop_and_signals_iterations() {
        let tempdir = tempdir().expect("temp dir");
        let path = tempdir.path().join("repair-db");
        let path_str = path.to_str().expect("path").to_string();
        let rt = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");

        rt.block_on(async move {
            let _guard = tempdir; // keep directory alive for the background task
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            install_iteration_hook(tx);
            spawn(path_str, Duration::from_millis(10));

            for _ in 0..2 {
                tokio::time::timeout(Duration::from_secs(1), rx.recv())
                    .await
                    .expect("timer")
                    .expect("iteration");
            }

            request_stop();
            tokio::time::sleep(Duration::from_millis(20)).await;
            clear_iteration_hook();
        });
    }
}
