use std::fmt;
use std::fs;
use std::io::{self, Result as IoResult};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::compute_market::settlement;
use crate::exec;
#[cfg(feature = "telemetry")]
use crate::telemetry::SUBSIDY_BYTES_TOTAL;
use crypto_suite::hashing::blake3;
use serde::{Deserialize, Serialize};

use foundation_serialization::binary;

use diagnostics::log;

use crate::legacy_cbor;

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
    let seq = next_sequence(&dir)?;
    let path = dir.join(format!("{}.bin", seq));
    let receipt = ReadReceipt {
        domain: domain.to_owned(),
        provider_id: provider_id.to_owned(),
        bytes_served,
        ts,
        dynamic,
        allowed,
    };
    if allowed {
        #[cfg(feature = "telemetry")]
        {
            SUBSIDY_BYTES_TOTAL
                .with_label_values(&["read"])
                .inc_by(bytes_served);
            crate::telemetry::READ_STATS.record(domain, bytes_served);
        }
    }
    let data = binary::encode(&receipt).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    fs::write(path, data)
}

pub fn batch(epoch: u64) -> IoResult<[u8; 32]> {
    let dir = base_dir().join("read").join(epoch.to_string());
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
    let root_dir = base_dir().join("read");
    fs::create_dir_all(&root_dir)?;
    let root_path = root_dir.join(format!("{}.root", epoch));
    fs::write(&root_path, encode_hex(root.as_bytes()))?;
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
                        if is_receipt_file(f.path().extension().and_then(|s| s.to_str())) {
                            match load_receipt(&f.path()) {
                                Ok(receipt) => {
                                    if receipt.domain == domain && receipt.allowed {
                                        total += 1;
                                        if receipt.ts > last {
                                            last = receipt.ts;
                                        }
                                    }
                                }
                                Err(err) => {
                                    log::warn!(
                                        "read_receipt_decode_failed path={} err={}",
                                        f.path().display(),
                                        err
                                    );
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

fn is_receipt_file(ext: Option<&str>) -> bool {
    matches!(ext, Some("cbor") | Some("bin"))
}

fn next_sequence(dir: &PathBuf) -> IoResult<u64> {
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

fn decode_legacy_receipt(bytes: &[u8]) -> Result<ReadReceipt, legacy_cbor::Error> {
    let value = legacy_cbor::parse(bytes)?;
    let map = value
        .as_map()
        .ok_or_else(|| legacy_cbor::Error::invalid_type("receipt root must be a map"))?;
    let domain = map
        .get("domain")
        .and_then(legacy_cbor::Value::as_text)
        .ok_or_else(|| legacy_cbor::Error::invalid_type("domain must be text"))?
        .to_owned();
    let provider_id = map
        .get("provider_id")
        .and_then(legacy_cbor::Value::as_text)
        .ok_or_else(|| legacy_cbor::Error::invalid_type("provider_id must be text"))?
        .to_owned();
    let bytes_served = map
        .get("bytes_served")
        .and_then(legacy_cbor::Value::as_u64)
        .ok_or_else(|| legacy_cbor::Error::invalid_type("bytes_served must be u64"))?;
    let ts = map
        .get("ts")
        .and_then(legacy_cbor::Value::as_u64)
        .ok_or_else(|| legacy_cbor::Error::invalid_type("ts must be u64"))?;
    let dynamic = map
        .get("dynamic")
        .and_then(legacy_cbor::Value::as_bool)
        .ok_or_else(|| legacy_cbor::Error::invalid_type("dynamic must be bool"))?;
    let allowed = map
        .get("allowed")
        .and_then(legacy_cbor::Value::as_bool)
        .ok_or_else(|| legacy_cbor::Error::invalid_type("allowed must be bool"))?;
    Ok(ReadReceipt {
        domain,
        provider_id,
        bytes_served,
        ts,
        dynamic,
        allowed,
    })
}

#[derive(Debug)]
enum ReceiptDecodeError {
    Io(io::Error),
    LegacyFallback {
        binary: foundation_serialization::Error,
        legacy: legacy_cbor::Error,
    },
}

impl fmt::Display for ReceiptDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReceiptDecodeError::Io(err) => write!(f, "io error: {err}"),
            ReceiptDecodeError::LegacyFallback { binary, legacy } => {
                write!(
                    f,
                    "decode failed (binary: {binary}; legacy fallback: {legacy})"
                )
            }
        }
    }
}

impl std::error::Error for ReceiptDecodeError {}

fn load_receipt(path: &Path) -> Result<ReadReceipt, ReceiptDecodeError> {
    let bytes = fs::read(path).map_err(ReceiptDecodeError::Io)?;
    match binary::decode::<ReadReceipt>(&bytes) {
        Ok(receipt) => Ok(receipt),
        Err(binary_err) => match decode_legacy_receipt(&bytes) {
            Ok(receipt) => Ok(receipt),
            Err(legacy_err) => Err(ReceiptDecodeError::LegacyFallback {
                binary: binary_err,
                legacy: legacy_err,
            }),
        },
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(TABLE[(b >> 4) as usize] as char);
        out.push(TABLE[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use foundation_serialization::binary;
    use sys::tempfile;

    #[test]
    fn decode_legacy_read_receipt() {
        let legacy = [
            0xA6, // map of six entries
            0x66, b'd', b'o', b'm', b'a', b'i', b'n', 0x6B, b'e', b'x', b'a', b'm', b'p', b'l',
            b'e', b'.', b'c', b'o', b'm', 0x6B, b'p', b'r', b'o', b'v', b'i', b'd', b'e', b'r',
            b'_', b'i', b'd', 0x6C, b'p', b'r', b'o', b'v', b'i', b'd', b'e', b'r', b'-', b'1',
            b'2', b'3', 0x6C, b'b', b'y', b't', b'e', b's', b'_', b's', b'e', b'r', b'v', b'e',
            b'd', 0x19, 0x02, 0x00, // 512
            0x62, b't', b's', 0x1A, 0x49, 0x96, 0x02, 0xD2, // 1_234_567_890
            0x67, b'd', b'y', b'n', b'a', b'm', b'i', b'c', 0xF5, // true
            0x67, b'a', b'l', b'l', b'o', b'w', b'e', b'd', 0xF4, // false
        ];

        let receipt = decode_legacy_receipt(&legacy).expect("decode legacy receipt");
        assert_eq!(receipt.domain, "example.com");
        assert_eq!(receipt.provider_id, "provider-123");
        assert_eq!(receipt.bytes_served, 512);
        assert_eq!(receipt.ts, 1_234_567_890);
        assert!(receipt.dynamic);
        assert!(!receipt.allowed);
    }

    #[test]
    fn read_receipt_from_binary_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let receipt = ReadReceipt {
            domain: "example.com".into(),
            provider_id: "provider-1".into(),
            bytes_served: 42,
            ts: 123,
            dynamic: true,
            allowed: true,
        };
        let bytes = binary::encode(&receipt).expect("encode");
        let path = dir.path().join("test.bin");
        fs::write(&path, bytes).expect("write");

        let decoded = load_receipt(&path).expect("decode binary");
        assert_eq!(decoded.domain, receipt.domain);
        assert_eq!(decoded.provider_id, receipt.provider_id);
        assert_eq!(decoded.bytes_served, receipt.bytes_served);
        assert_eq!(decoded.ts, receipt.ts);
        assert_eq!(decoded.dynamic, receipt.dynamic);
        assert_eq!(decoded.allowed, receipt.allowed);
    }

    #[test]
    fn read_receipt_from_legacy_file() {
        let legacy = [
            0xA6, // map of six entries
            0x66, b'd', b'o', b'm', b'a', b'i', b'n', 0x6B, b'e', b'x', b'a', b'm', b'p', b'l',
            b'e', b'.', b'c', b'o', b'm', 0x6B, b'p', b'r', b'o', b'v', b'i', b'd', b'e', b'r',
            b'_', b'i', b'd', 0x6C, b'p', b'r', b'o', b'v', b'i', b'd', b'e', b'r', b'-', b'1',
            b'2', b'3', 0x6C, b'b', b'y', b't', b'e', b's', b'_', b's', b'e', b'r', b'v', b'e',
            b'd', 0x19, 0x02, 0x00, // 512
            0x62, b't', b's', 0x1A, 0x49, 0x96, 0x02, 0xD2, // 1_234_567_890
            0x67, b'd', b'y', b'n', b'a', b'm', b'i', b'c', 0xF5, // true
            0x67, b'a', b'l', b'l', b'o', b'w', b'e', b'd', 0xF4, // false
        ];
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("legacy.cbor");
        fs::write(&path, &legacy).expect("write legacy");

        let decoded = load_receipt(&path).expect("decode legacy");
        assert_eq!(decoded.domain, "example.com");
        assert!(!decoded.allowed);
    }
}
