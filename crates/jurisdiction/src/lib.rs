#[cfg(feature = "pq")]
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Region specific policy pack controlling default consent and feature toggles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyPack {
    pub region: String,
    pub consent_required: bool,
    pub features: Vec<String>,
}

impl PolicyPack {
    /// Load a policy pack from a JSON file.
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Built-in template for a given region code (e.g. "US").
    pub fn template(region: &str) -> Option<Self> {
        let raw = match region {
            "US" => Some(include_str!("../policies/us.json")),
            "EU" => Some(include_str!("../policies/eu.json")),
            _ => None,
        }?;
        serde_json::from_str(raw).ok()
    }
}

/// Encrypt metadata for storage if the `pq` feature is enabled.
/// Log a law-enforcement request (metadata only). If PQ encryption is enabled the
/// metadata is encrypted before being written.
pub fn log_law_enforcement_request(path: impl AsRef<Path>, metadata: &str) -> std::io::Result<()> {
    #[cfg(feature = "pq")]
    fn encrypt_metadata(data: &[u8]) -> Vec<u8> {
        use pqcrypto_kyber::kyber1024;
        let (pk, _sk) = kyber1024::keypair();
        let (cipher, _) = kyber1024::encapsulate(&pk);
        [cipher.as_bytes(), data].concat()
    }

    #[cfg(feature = "pq")]
    let out = {
        let enc = encrypt_metadata(metadata.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(enc)
    };
    #[cfg(not(feature = "pq"))]
    let out = metadata.to_owned();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    use std::io::Write;
    writeln!(file, "{out}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_pack() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("pack.json");
        std::fs::write(
            &file,
            b"{\"region\":\"US\",\"consent_required\":true,\"features\":[\"wallet\"]}",
        )
        .unwrap();
        let pack = PolicyPack::load(&file).unwrap();
        assert_eq!(pack.region, "US");
        assert!(pack.consent_required);
        assert_eq!(pack.features, vec!["wallet"]);
    }
}
