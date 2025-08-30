use super::erasure;
use super::types::{ObjectManifest, Redundancy};
use crate::simple_db::SimpleDb;
#[cfg(feature = "telemetry")]
use crate::telemetry::{STORAGE_REPAIR_BYTES_TOTAL, STORAGE_REPAIR_FAILURES_TOTAL};
use std::time::Duration;

pub fn spawn(path: String, period: Duration) {
    tokio::spawn(async move {
        let mut db = SimpleDb::open(&path);
        let mut tick = tokio::time::interval(period);
        loop {
            tick.tick().await;
            if let Err(_) = run_once(&mut db) {
                #[cfg(feature = "telemetry")]
                STORAGE_REPAIR_FAILURES_TOTAL.inc();
            }
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
