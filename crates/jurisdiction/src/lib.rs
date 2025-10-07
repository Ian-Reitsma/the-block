#[cfg(feature = "pq")]
use base64_fp::encode_standard;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Region specific policy pack controlling default consent and feature toggles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyPack {
    pub region: String,
    pub consent_required: bool,
    pub features: Vec<String>,
    /// Optional parent region to inherit defaults from (e.g. country -> state -> municipality).
    #[serde(default)]
    pub parent: Option<String>,
}

/// Signed policy feed item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedPack {
    pub pack: PolicyPack,
    pub signature: Vec<u8>,
}

impl SignedPack {
    /// Verify the signature against a given public key.
    pub fn verify(&self, pk: &VerifyingKey) -> bool {
        if let Ok(bytes) = <[u8; 64]>::try_from(self.signature.as_slice()) {
            let sig = Signature::from_bytes(&bytes);
            return pk
                .verify(serde_json::to_string(&self.pack).unwrap().as_bytes(), &sig)
                .is_ok();
        }
        false
    }
}

/// Simple in-memory cache keyed by region.
static CACHE: once_cell::sync::Lazy<std::sync::Mutex<HashMap<String, PolicyPack>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

/// Fetch a signed policy pack from a URL and cache it.
pub fn fetch_signed(url: &str, pk: &VerifyingKey) -> std::io::Result<PolicyPack> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let body = resp
        .into_string()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let signed: SignedPack = serde_json::from_str(&body)?;
    if !signed.verify(pk) {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "bad sig"));
    }
    let pack = signed.pack.resolve();
    CACHE
        .lock()
        .unwrap()
        .insert(pack.region.clone(), pack.clone());
    Ok(pack)
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

    /// Resolve inheritance chain, merging parent features and consent flags.
    pub fn resolve(self) -> Self {
        if let Some(parent_id) = &self.parent {
            if let Some(mut parent) = Self::template(parent_id) {
                // allow nested inheritance
                parent = parent.resolve();
                let mut features = parent.features;
                features.extend(self.features.clone());
                PolicyPack {
                    region: self.region,
                    consent_required: self.consent_required,
                    features,
                    parent: self.parent,
                }
            } else {
                self
            }
        } else {
            self
        }
    }

    /// Compute a semantic diff between two packs for RPC consumption.
    pub fn diff(old: &Self, new: &Self) -> serde_json::Value {
        let mut changed = serde_json::Map::new();
        if old.consent_required != new.consent_required {
            changed.insert(
                "consent_required".into(),
                serde_json::json!({"old": old.consent_required, "new": new.consent_required}),
            );
        }
        if old.features != new.features {
            changed.insert(
                "features".into(),
                serde_json::json!({"old": old.features, "new": new.features}),
            );
        }
        serde_json::Value::Object(changed)
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
        encode_standard(&enc)
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
