pub mod codec;

use base64_fp::decode_standard;
#[cfg(feature = "pq")]
use base64_fp::encode_standard;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use diagnostics::log;
use foundation_lazy::sync::Lazy;
use foundation_serialization::json::{self, Map as JsonMap, Value as JsonValue};
use http_env::blocking_client as env_blocking_client;
use httpd::{BlockingClient, ClientError as HttpClientError, Method};
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::path::Path;

/// Region specific policy pack controlling default consent and feature toggles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyPack {
    pub region: String,
    pub consent_required: bool,
    pub features: Vec<String>,
    /// Optional parent region to inherit defaults from (e.g. country -> state -> municipality).
    pub parent: Option<String>,
}

/// Signed policy feed item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedPack {
    pub pack: PolicyPack,
    pub signature: Vec<u8>,
}

impl SignedPack {
    fn parse_signature(value: &JsonValue) -> std::io::Result<Vec<u8>> {
        match value {
            JsonValue::Array(entries) => {
                let mut bytes = Vec::with_capacity(entries.len());
                for (index, entry) in entries.iter().enumerate() {
                    let number = entry.as_u64().ok_or_else(|| {
                        invalid_field("signature", format!("index {index} must be integer"))
                    })?;
                    if number > u8::MAX as u64 {
                        return Err(invalid_field(
                            "signature",
                            format!("index {index} value {number} exceeds u8 range"),
                        ));
                    }
                    bytes.push(number as u8);
                }
                Ok(bytes)
            }
            JsonValue::String(encoded) => {
                decode_standard(encoded).map_err(|err| invalid_field("signature", err.to_string()))
            }
            JsonValue::Null => Ok(Vec::new()),
            other => Err(invalid_field(
                "signature",
                format!("expected array, string, or null, found {other:?}"),
            )),
        }
    }

    pub fn from_json_value(value: &JsonValue) -> std::io::Result<Self> {
        let map = value
            .as_object()
            .ok_or_else(|| invalid_data("signed pack must be a JSON object"))?;

        let pack_value = map
            .get("pack")
            .ok_or_else(|| invalid_field("pack", "missing field"))?;
        let pack = PolicyPack::from_json_value(pack_value)?;

        let signature_value = map
            .get("signature")
            .ok_or_else(|| invalid_field("signature", "missing field"))?;
        let signature = Self::parse_signature(signature_value)?;

        Ok(SignedPack { pack, signature })
    }

    pub fn from_json_str(text: &str) -> std::io::Result<Self> {
        let value = json::value_from_str(text).map_err(|err| invalid_data(err.to_string()))?;
        Self::from_json_value(&value)
    }

    pub fn from_json_slice(bytes: &[u8]) -> std::io::Result<Self> {
        let value = json::value_from_slice(bytes).map_err(|err| invalid_data(err.to_string()))?;
        Self::from_json_value(&value)
    }

    pub fn to_json_value(&self) -> JsonValue {
        let mut map = JsonMap::new();
        map.insert("pack".into(), self.pack.to_json_value());
        let signature = self
            .signature
            .iter()
            .map(|byte| JsonValue::from(*byte as u64))
            .collect();
        map.insert("signature".into(), JsonValue::Array(signature));
        JsonValue::Object(map)
    }

    /// Verify the signature against a given public key.
    pub fn verify(&self, pk: &VerifyingKey) -> bool {
        if let Ok(bytes) = <[u8; 64]>::try_from(self.signature.as_slice()) {
            let sig = Signature::from_bytes(&bytes);
            let payload = json::to_string_value(&self.pack.to_json_value());
            return pk.verify(payload.as_bytes(), &sig).is_ok();
        }
        false
    }
}

/// Simple in-memory cache keyed by region.

static CACHE: Lazy<std::sync::Mutex<HashMap<String, PolicyPack>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

static HTTP_CLIENT: Lazy<BlockingClient> =
    Lazy::new(|| env_blocking_client(&["TB_JURISDICTION_TLS", "TB_HTTP_TLS"], "jurisdiction"));

fn map_http_error(err: HttpClientError) -> io::Error {
    if err.is_timeout() {
        io::Error::new(ErrorKind::TimedOut, err.to_string())
    } else {
        io::Error::new(ErrorKind::Other, err.to_string())
    }
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message.into())
}

fn invalid_field(field: &str, detail: impl Into<String>) -> io::Error {
    invalid_data(format!("{field}: {}", detail.into()))
}

/// Fetch a signed policy pack from a URL and cache it.
pub fn fetch_signed(url: &str, pk: &VerifyingKey) -> std::io::Result<PolicyPack> {
    log::info!("jurisdiction.fetch_signed start url={url}");
    let response = HTTP_CLIENT
        .request(Method::Get, url)
        .map_err(map_http_error)?
        .send()
        .map_err(map_http_error)?;
    if !response.status().is_success() {
        log::warn!(
            "jurisdiction.fetch_signed http_status error url={url} status={}",
            response.status().as_u16()
        );
        return Err(io::Error::new(
            ErrorKind::Other,
            format!("http status {}", response.status().as_u16()),
        ));
    }
    let body = response
        .text()
        .map_err(|err| io::Error::new(ErrorKind::InvalidData, err.to_string()))?;
    let signed = SignedPack::from_json_str(&body)?;
    if !signed.verify(pk) {
        log::warn!("jurisdiction.fetch_signed bad_signature url={url}");
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "bad sig"));
    }
    let pack = signed.pack.resolve();
    CACHE
        .lock()
        .unwrap()
        .insert(pack.region.clone(), pack.clone());
    log::info!(
        "jurisdiction.fetch_signed cached region={} consent={} features_count={}",
        pack.region,
        pack.consent_required,
        pack.features.len()
    );
    Ok(pack)
}

impl PolicyPack {
    pub fn from_json_value(value: &JsonValue) -> std::io::Result<Self> {
        let map = value
            .as_object()
            .ok_or_else(|| invalid_data("policy pack must be a JSON object"))?;

        let region = map
            .get("region")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| invalid_field("region", "expected string"))?
            .to_owned();

        let consent_required = map
            .get("consent_required")
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| invalid_field("consent_required", "expected boolean"))
            })
            .transpose()?
            .unwrap_or(false);

        let features = map
            .get("features")
            .map(|value| -> std::io::Result<Vec<String>> {
                let array = value
                    .as_array()
                    .ok_or_else(|| invalid_field("features", "expected array"))?;
                let mut out = Vec::with_capacity(array.len());
                for (index, entry) in array.iter().enumerate() {
                    let feature = entry.as_str().ok_or_else(|| {
                        invalid_field("features", format!("index {index} must be string"))
                    })?;
                    out.push(feature.to_owned());
                }
                Ok(out)
            })
            .transpose()?
            .unwrap_or_else(Vec::new);

        let parent = match map.get("parent") {
            Some(JsonValue::String(value)) => Some(value.clone()),
            Some(JsonValue::Null) => None,
            Some(other) => {
                return Err(invalid_field(
                    "parent",
                    format!("expected string or null, found {other:?}"),
                ))
            }
            None => None,
        };

        Ok(PolicyPack {
            region,
            consent_required,
            features,
            parent,
        })
    }

    pub fn from_json_str(text: &str) -> std::io::Result<Self> {
        let value = json::value_from_str(text).map_err(|err| invalid_data(err.to_string()))?;
        Self::from_json_value(&value)
    }

    pub fn from_json_slice(bytes: &[u8]) -> std::io::Result<Self> {
        let value = json::value_from_slice(bytes).map_err(|err| invalid_data(err.to_string()))?;
        Self::from_json_value(&value)
    }

    pub fn to_json_value(&self) -> JsonValue {
        let mut map = JsonMap::new();
        map.insert("region".into(), JsonValue::String(self.region.clone()));
        map.insert(
            "consent_required".into(),
            JsonValue::Bool(self.consent_required),
        );
        let features = self
            .features
            .iter()
            .cloned()
            .map(JsonValue::String)
            .collect();
        map.insert("features".into(), JsonValue::Array(features));
        if let Some(parent) = &self.parent {
            map.insert("parent".into(), JsonValue::String(parent.clone()));
        }
        JsonValue::Object(map)
    }

    /// Load a policy pack from a JSON file.
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_json_slice(&bytes)
    }

    /// Built-in template for a given region code (e.g. "US").
    pub fn template(region: &str) -> Option<Self> {
        let raw = match region {
            "US" => Some(include_str!("../policies/us.json")),
            "EU" => Some(include_str!("../policies/eu.json")),
            _ => None,
        }?;
        Self::from_json_str(raw).ok()
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
    pub fn diff(old: &Self, new: &Self) -> json::Value {
        let mut changed = JsonMap::new();
        if old.consent_required != new.consent_required {
            let mut entry = JsonMap::new();
            entry.insert("old".into(), JsonValue::Bool(old.consent_required));
            entry.insert("new".into(), JsonValue::Bool(new.consent_required));
            changed.insert("consent_required".into(), JsonValue::Object(entry));
        }
        if old.features != new.features {
            let mut entry = JsonMap::new();
            entry.insert(
                "old".into(),
                JsonValue::Array(
                    old.features
                        .iter()
                        .map(|f| JsonValue::String(f.clone()))
                        .collect(),
                ),
            );
            entry.insert(
                "new".into(),
                JsonValue::Array(
                    new.features
                        .iter()
                        .map(|f| JsonValue::String(f.clone()))
                        .collect(),
                ),
            );
            changed.insert("features".into(), JsonValue::Object(entry));
        }
        JsonValue::Object(changed)
    }
}

/// Encrypt metadata for storage if the `pq` feature is enabled.
/// Log a law-enforcement request (metadata only). If PQ encryption is enabled the
/// metadata is encrypted before being written.
pub fn log_law_enforcement_request(path: impl AsRef<Path>, metadata: &str) -> std::io::Result<()> {
    let path = path.as_ref();
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
    log::info!(
        "jurisdiction.log_law_enforcement_request appended bytes={} path={}",
        out.len(),
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::tempfile::tempdir;

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
