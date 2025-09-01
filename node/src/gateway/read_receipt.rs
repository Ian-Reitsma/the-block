use std::fs;
use std::io::Result as IoResult;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::exec;
use crate::compute_market::settlement;
use blake3;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReadReceipt {
    pub domain: String,
    pub provider_id: String,
    pub bytes_served: u64,
    pub ts: u64,
    pub dynamic: bool,
    pub allowed: bool,
}

fn base_dir() -> PathBuf {
    std::env::var("TB_GATEWAY_RECEIPTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("receipts"))
}

fn current_epoch(ts: u64) -> u64 {
    ts / 3600
}

pub fn append(
    domain: &str,
    provider_id: &str,
    bytes_served: u64,
    dynamic: bool,
    allowed: bool,
) -> IoResult<()> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let epoch = current_epoch(ts);
    let dir = base_dir().join("read").join(epoch.to_string());
    fs::create_dir_all(&dir)?;
    let seq = fs::read_dir(&dir)?
        .filter(|e| {
            e.as_ref()
                .ok()
                .and_then(|f| {
                    f.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|ext| ext == "cbor")
                })
                .unwrap_or(false)
        })
        .count();
    let path = dir.join(format!("{}.cbor", seq));
    let receipt = ReadReceipt {
        domain: domain.to_owned(),
        provider_id: provider_id.to_owned(),
        bytes_served,
        ts,
        dynamic,
        allowed,
    };
    let data = serde_cbor::to_vec(&receipt)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, data)
}

pub fn batch(epoch: u64) -> IoResult<[u8; 32]> {
    let dir = base_dir().join("read").join(epoch.to_string());
    let mut hashes = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for ent in entries.flatten() {
            if ent.path().extension().and_then(|s| s.to_str()) == Some("cbor") {
                if let Ok(bytes) = fs::read(ent.path()) {
                    hashes.push(blake3::hash(&bytes));
                }
            }
        }
    }
    hashes.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut root = blake3::Hash::from([0u8; 32]);
    for h in hashes {
        let mut hasher = blake3::Hasher::new();
        hasher.update(root.as_bytes());
        hasher.update(h.as_bytes());
        root = hasher.finalize();
    }
    let root_dir = base_dir().join("read");
    fs::create_dir_all(&root_dir)?;
    let root_path = root_dir.join(format!("{}.root", epoch));
    fs::write(&root_path, hex::encode(root.as_bytes()))?;
    let exec_root = exec::batch(epoch)?;
    let mut anchor_hasher = blake3::Hasher::new();
    anchor_hasher.update(root.as_bytes());
    anchor_hasher.update(&exec_root);
    let anchor = anchor_hasher.finalize();
    settlement::submit_anchor(anchor.as_bytes());
    Ok(*anchor.as_bytes())
}

pub fn reads_since(epoch: u64, domain: &str) -> (u64, u64) {
    let mut total = 0;
    let mut last = 0;
    let base = base_dir().join("read");
    if let Ok(entries) = fs::read_dir(&base) {
        for ent in entries.flatten() {
            if let Ok(e) = ent.file_name().to_string_lossy().parse::<u64>() {
                if e < epoch {
                    continue;
                }
                if let Ok(files) = fs::read_dir(ent.path()) {
                    for f in files.flatten() {
                        if f.path().extension().and_then(|s| s.to_str()) == Some("cbor") {
                            if let Ok(bytes) = fs::read(f.path()) {
                                if let Ok(r) = serde_cbor::from_slice::<ReadReceipt>(&bytes) {
                                    if r.domain == domain && r.allowed {
                                        total += 1;
                                        if r.ts > last {
                                            last = r.ts;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (total, last)
}
