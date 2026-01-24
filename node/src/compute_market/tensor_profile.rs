use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::hex;
use crypto_suite::mac::sha256_digest;
#[cfg(feature = "telemetry")]
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TensorProfileSnapshot {
    pub epoch: String,
    #[cfg(feature = "telemetry")]
    pub delta: i64,
    #[cfg(feature = "telemetry")]
    pub label_deltas: Vec<(String, i64)>,
}

pub fn capture_tensor_profile_snapshot() -> Option<TensorProfileSnapshot> {
    const LOG_PATH: &str = "/tmp/orchard_tensor_profile.log";
    let path = Path::new(LOG_PATH);
    let metadata = path.metadata().ok()?;
    let modified = metadata.modified().ok()?;
    let timestamp = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }

    let mut alloc = 0u64;
    let mut free = 0u64;
    #[cfg(feature = "telemetry")]
    let mut label_counts = BTreeMap::new();
    for line in bytes.split(|b| *b == b'\n') {
        if line.starts_with(b"alloc ") {
            alloc = alloc.saturating_add(1);
            #[cfg(feature = "telemetry")]
            if let Some(label) = parse_label(line) {
                *label_counts.entry(label).or_insert(0) += 1;
            }
        } else if line.starts_with(b"free ") {
            free = free.saturating_add(1);
            #[cfg(feature = "telemetry")]
            if let Some(label) = parse_label(line) {
                *label_counts.entry(label).or_insert(0) -= 1;
            }
        }
    }

    #[cfg(feature = "telemetry")]
    let delta = alloc as i64 - free as i64;
    #[cfg(feature = "telemetry")]
    let label_deltas = label_counts
        .into_iter()
        .filter(|(_, value)| *value != 0)
        .collect::<Vec<_>>();

    let mut hasher = Hasher::new();
    hasher.update(&bytes);
    hasher.update(&timestamp.to_le_bytes());
    hasher.update(&alloc.to_le_bytes());
    hasher.update(&free.to_le_bytes());
    let digest = hasher.finalize();
    let digest_hex = hex::encode(digest.as_bytes());

    let epoch = format!(
        "orchard-profile:{}:{}:{}:{}",
        timestamp, alloc, free, digest_hex
    );
    #[cfg(feature = "telemetry")]
    {
        Some(TensorProfileSnapshot {
            epoch,
            delta,
            label_deltas,
        })
    }
    #[cfg(not(feature = "telemetry"))]
    {
        Some(TensorProfileSnapshot { epoch })
    }
}

#[cfg(feature = "telemetry")]
fn parse_label(line: &[u8]) -> Option<String> {
    let mut parts = line.split(|b| *b == b' ');
    parts.next()?;
    parts
        .next()
        .map(|label_bytes| String::from_utf8_lossy(label_bytes).trim_end().to_string())
}
