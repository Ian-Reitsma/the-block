#![doc = "Utilities for hashing TLS environment warning payloads and generating stable labels."]

use crypto_suite::hashing::blake3;
use foundation_serialization::serde::{Deserialize, Serialize};

/// Delimiter used when hashing warning variable lists.
pub const VARIABLE_DELIMITER: u8 = 0x1f;

/// Compute the canonical fingerprint for the provided detail payload.
#[inline]
pub fn detail_fingerprint(detail: &str) -> i64 {
    fingerprint_from_bytes(detail.as_bytes())
}

/// Compute the canonical fingerprint for an iterator of TLS warning variables.
///
/// Empty iterators return `None` so callers can differentiate "no variables"
/// from a hashed payload.
pub fn variables_fingerprint<I, S>(variables: I) -> Option<i64>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut bytes = Vec::new();
    let mut first = true;
    for value in variables {
        let value = value.as_ref();
        if value.is_empty() {
            continue;
        }
        if first {
            first = false;
        } else {
            bytes.push(VARIABLE_DELIMITER);
        }
        bytes.extend_from_slice(value.as_bytes());
    }

    if first {
        None
    } else {
        Some(fingerprint_from_bytes(&bytes))
    }
}

/// Format the optional fingerprint as a lowercase hexadecimal label suitable
/// for Prometheus metrics.
#[inline]
pub fn fingerprint_label(fingerprint: Option<i64>) -> String {
    fingerprint
        .map(|value| {
            let unsigned = u64::from_le_bytes(value.to_le_bytes());
            format!("{unsigned:016x}")
        })
        .unwrap_or_else(|| "none".to_string())
}

/// Raw helper exposed so callers that already normalised byte payloads can
/// avoid re-allocating intermediate strings.
#[inline]
pub fn fingerprint_from_bytes(bytes: &[u8]) -> i64 {
    let digest = blake3::hash(bytes);
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&digest.as_bytes()[..8]);
    i64::from_le_bytes(buf)
}

/// Identifies the source that emitted the TLS environment warning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde", rename_all = "snake_case")]
pub enum WarningOrigin {
    Diagnostics,
    PeerIngest,
}

impl WarningOrigin {
    /// Return the canonical label string for the origin.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            WarningOrigin::Diagnostics => "diagnostics",
            WarningOrigin::PeerIngest => "peer_ingest",
        }
    }

    /// Parse the origin from a canonical label string.
    pub fn from_str(label: &str) -> Option<Self> {
        match label {
            "diagnostics" => Some(WarningOrigin::Diagnostics),
            "peer_ingest" => Some(WarningOrigin::PeerIngest),
            _ => None,
        }
    }
}

impl Default for WarningOrigin {
    fn default() -> Self {
        WarningOrigin::PeerIngest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_hash_matches_direct_bytes() {
        let payload = "identity requires TB_NODE_TLS_CERT";
        assert_eq!(
            detail_fingerprint(payload),
            fingerprint_from_bytes(payload.as_bytes())
        );
    }

    #[test]
    fn variables_hash_ignores_empty_values() {
        let fingerprint =
            variables_fingerprint(["TB_NODE_TLS_CERT", "TB_NODE_TLS_KEY", ""]).unwrap();
        let control = variables_fingerprint(["TB_NODE_TLS_CERT", "TB_NODE_TLS_KEY"]).unwrap();
        assert_eq!(fingerprint, control);
    }

    #[test]
    fn variables_hash_returns_none_for_empty_iter() {
        assert!(variables_fingerprint::<Vec<&str>, &str>(Vec::<&str>::new()).is_none());
    }

    #[test]
    fn fingerprint_label_formats_hex() {
        let value = detail_fingerprint("detail");
        let label = fingerprint_label(Some(value));
        let expected = format!("{:016x}", u64::from_le_bytes(value.to_le_bytes()));
        assert_eq!(label, expected);
        assert_eq!(fingerprint_label(None), "none");
    }

    #[test]
    fn origin_round_trips_from_label() {
        for origin in [WarningOrigin::Diagnostics, WarningOrigin::PeerIngest] {
            let label = origin.as_str();
            assert_eq!(WarningOrigin::from_str(label), Some(origin));
        }
        assert_eq!(WarningOrigin::from_str("unknown"), None);
    }
}
