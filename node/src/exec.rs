use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    fs,
    io::{self, Result as IoResult},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::compute_market::settlement;
#[cfg(feature = "telemetry")]
use crate::telemetry::{SUBSIDY_BYTES_TOTAL, SUBSIDY_CPU_MS_TOTAL};
use crypto_suite::hashing::blake3::{self, Hasher};
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_bytes;

use foundation_serialization::binary;

static CPU_MS: AtomicU64 = AtomicU64::new(0);
static BYTES_OUT: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExecReceipt {
    pub provider_id: String,
    pub func: [u8; 32],
    pub cpu_ms: u64,
    pub bytes_out: u64,
    pub ts: u64,
    pub pk: [u8; 32],
    #[serde(with = "serde_bytes")]
    pub sig: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub auditor_sig: Vec<u8>,
}

impl ExecReceipt {
    pub fn verify(&self) -> bool {
        if self.pk == [0u8; 32] {
            return true;
        }
        let mut h = Hasher::new();
        h.update(&self.func);
        h.update(&self.bytes_out.to_le_bytes());
        h.update(&self.cpu_ms.to_le_bytes());
        h.update(&self.ts.to_le_bytes());
        let msg = h.finalize();
        let pk = match VerifyingKey::from_bytes(&self.pk) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let arr: [u8; 64] = match self.sig.as_slice().try_into() {
            Ok(a) => a,
            Err(_) => return false,
        };
        let sig = Signature::from_bytes(&arr);
        pk.verify(msg.as_bytes(), &sig).is_ok()
    }
}

fn base_dir() -> PathBuf {
    std::env::var("TB_GATEWAY_RECEIPTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("receipts"))
        .join("exec")
}

fn current_epoch(ts: u64) -> u64 {
    ts / 3600
}

fn append_exec(r: &ExecReceipt, epoch: u64, seq: u64) -> IoResult<()> {
    let dir = base_dir().join(epoch.to_string());
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.bin", seq));
    let data = binary::encode(r).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    fs::write(path, data)
}

pub fn record(
    provider_id: &str,
    func: [u8; 32],
    bytes_out: u64,
    cpu_ms: u64,
    pk: [u8; 32],
    sig: Vec<u8>,
    auditor_sig: Vec<u8>,
    trace_id: &str,
) -> IoResult<()> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let receipt = ExecReceipt {
        provider_id: provider_id.to_owned(),
        func,
        cpu_ms,
        bytes_out,
        ts,
        pk,
        sig,
        auditor_sig,
    };
    if !receipt.verify() {
        let _ = settlement::Settlement::penalize_sla(provider_id, cpu_ms / 1000);
        return Ok(());
    }
    let epoch = current_epoch(ts);
    let dir = base_dir().join(epoch.to_string());
    fs::create_dir_all(&dir)?;
    let seq = next_sequence(&dir)?;
    append_exec(&receipt, epoch, seq)?;
    CPU_MS.fetch_add(cpu_ms, Ordering::Relaxed);
    BYTES_OUT.fetch_add(bytes_out, Ordering::Relaxed);
    #[cfg(not(feature = "telemetry"))]
    let _ = trace_id;
    #[cfg(feature = "telemetry")]
    diagnostics::tracing::info!(%trace_id, provider_id, bytes_out, cpu_ms, "exec receipt");
    #[cfg(feature = "telemetry")]
    {
        SUBSIDY_CPU_MS_TOTAL.inc_by(cpu_ms);
        SUBSIDY_BYTES_TOTAL
            .with_label_values(&["compute"])
            .inc_by(bytes_out);
    }
    Ok(())
}

pub fn batch(epoch: u64) -> IoResult<[u8; 32]> {
    let dir = base_dir().join(epoch.to_string());
    let mut hashes = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for ent in entries.flatten() {
            if is_receipt_file(ent.path().extension().and_then(|s| s.to_str())) {
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
    fs::create_dir_all(&base_dir())?;
    let root_path = base_dir().join(format!("{}.root", epoch));
    fs::write(&root_path, hex::encode(root.as_bytes()))?;
    Ok(*root.as_bytes())
}

pub fn take_metrics() -> (u64, u64) {
    let cpu = CPU_MS.swap(0, Ordering::Relaxed);
    let out = BYTES_OUT.swap(0, Ordering::Relaxed);
    (cpu, out)
}

fn is_receipt_file(ext: Option<&str>) -> bool {
    matches!(ext, Some("cbor") | Some("bin"))
}

fn next_sequence(dir: &Path) -> IoResult<u64> {
    let mut max_id: Option<u64> = None;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_receipt_file(path.extension().and_then(|s| s.to_str())) {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if let Ok(id) = stem.parse::<u64>() {
                    max_id = Some(max_id.map_or(id, |curr| curr.max(id)));
                }
            }
        }
    }
    Ok(max_id.map_or(0, |id| id + 1))
}
