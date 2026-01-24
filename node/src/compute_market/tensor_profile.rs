use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::hex;
use crypto_suite::mac::sha256_digest;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TensorProfileSnapshot {
    pub epoch: String,
    pub delta: i64,
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
    for line in bytes.split(|b| *b == b'\n') {
        if line.starts_with(b"alloc ") {
            alloc = alloc.saturating_add(1);
        } else if line.starts_with(b"free ") {
            free = free.saturating_add(1);
        }
    }

    let delta = alloc as i64 - free as i64;

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
    Some(TensorProfileSnapshot { epoch, delta })
}
