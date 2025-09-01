use std::fs;
use std::io::Result as IoResult;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use blake3;
use serde::{Deserialize, Serialize};

use super::read_receipt;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExecutionReceipt {
    pub domain: String,
    pub provider_id: String,
    pub cpu_seconds: u64,
    pub disk_io_bytes: u64,
    pub ts: u64,
}

fn base_dir() -> PathBuf {
    std::env::var("TB_GATEWAY_RECEIPTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("receipts"))
}

fn current_epoch(ts: u64) -> u64 {
    ts / 3600
}

fn append_exec(r: &ExecutionReceipt, epoch: u64, seq: usize) -> IoResult<()> {
    let dir = base_dir().join("exec").join(epoch.to_string());
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.cbor", seq));
    let data =
        serde_cbor::to_vec(r).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, data)
}

pub fn record(
    domain: &str,
    provider_id: &str,
    bytes_served: u64,
    cpu_seconds: u64,
    disk_io_bytes: u64,
) -> IoResult<()> {
    read_receipt::append(domain, provider_id, bytes_served, true, true)?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let epoch = current_epoch(ts);
    let dir = base_dir().join("exec").join(epoch.to_string());
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
    let receipt = ExecutionReceipt {
        domain: domain.to_owned(),
        provider_id: provider_id.to_owned(),
        cpu_seconds,
        disk_io_bytes,
        ts,
    };
    append_exec(&receipt, epoch, seq)
}

pub fn batch(epoch: u64) -> IoResult<[u8; 32]> {
    let dir = base_dir().join("exec").join(epoch.to_string());
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
    let root_dir = base_dir().join("exec");
    fs::create_dir_all(&root_dir)?;
    let root_path = root_dir.join(format!("{}.root", epoch));
    fs::write(&root_path, hex::encode(root.as_bytes()))?;
    Ok(*root.as_bytes())
}
