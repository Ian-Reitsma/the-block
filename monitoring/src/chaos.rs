#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3;
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey, SIGNATURE_LENGTH};
use foundation_serialization::json::{Map, Value};
use foundation_serialization::{Deserialize, Serialize};
use std::fmt;

/// Modules orchestrated by the WAN chaos harness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChaosModule {
    Overlay,
    Storage,
    Compute,
}

impl ChaosModule {
    pub fn as_str(self) -> &'static str {
        match self {
            ChaosModule::Overlay => "overlay",
            ChaosModule::Storage => "storage",
            ChaosModule::Compute => "compute",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "overlay" => Some(ChaosModule::Overlay),
            "storage" => Some(ChaosModule::Storage),
            "compute" => Some(ChaosModule::Compute),
            _ => None,
        }
    }
}

impl fmt::Display for ChaosModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Per-site readiness information tracked alongside module readiness.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChaosSiteReadiness {
    pub site: String,
    pub readiness: f64,
}

/// Unsigned readiness snapshot produced by the chaos harness.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChaosAttestationDraft {
    pub scenario: String,
    pub module: ChaosModule,
    pub readiness: f64,
    pub sla_threshold: f64,
    pub breaches: u64,
    pub window_start: u64,
    pub window_end: u64,
    pub issued_at: u64,
    #[serde(default)]
    pub site_readiness: Vec<ChaosSiteReadiness>,
}

impl ChaosAttestationDraft {
    pub fn normalize(mut self) -> Self {
        self.readiness = self.readiness.clamp(0.0, 1.0);
        self.sla_threshold = self.sla_threshold.clamp(0.0, 1.0);
        for site in &mut self.site_readiness {
            site.readiness = site.readiness.clamp(0.0, 1.0);
        }
        self.site_readiness.sort_by(|a, b| a.site.cmp(&b.site));
        self
    }

    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.scenario.as_bytes());
        hasher.update(self.module.as_str().as_bytes());
        hasher.update(&self.readiness.to_le_bytes());
        hasher.update(&self.sla_threshold.to_le_bytes());
        hasher.update(&self.breaches.to_le_bytes());
        hasher.update(&self.window_start.to_le_bytes());
        hasher.update(&self.window_end.to_le_bytes());
        hasher.update(&self.issued_at.to_le_bytes());
        hasher.update(&(self.site_readiness.len() as u64).to_le_bytes());
        for entry in &self.site_readiness {
            let name_bytes = entry.site.as_bytes();
            hasher.update(&(name_bytes.len() as u64).to_le_bytes());
            hasher.update(name_bytes);
            hasher.update(&entry.readiness.to_le_bytes());
        }
        *hasher.finalize().as_bytes()
    }
}

/// Cryptographically signed readiness attestation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChaosAttestation {
    pub scenario: String,
    pub module: ChaosModule,
    pub readiness: f64,
    pub sla_threshold: f64,
    pub breaches: u64,
    pub window_start: u64,
    pub window_end: u64,
    pub issued_at: u64,
    pub signer: [u8; 32],
    pub signature: [u8; SIGNATURE_LENGTH],
    pub digest: [u8; 32],
    #[serde(default)]
    pub site_readiness: Vec<ChaosSiteReadiness>,
}

impl ChaosAttestation {
    pub fn draft(&self) -> ChaosAttestationDraft {
        ChaosAttestationDraft {
            scenario: self.scenario.clone(),
            module: self.module,
            readiness: self.readiness,
            sla_threshold: self.sla_threshold,
            breaches: self.breaches,
            window_start: self.window_start,
            window_end: self.window_end,
            issued_at: self.issued_at,
            site_readiness: self.site_readiness.clone(),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("scenario".into(), Value::String(self.scenario.clone()));
        map.insert(
            "module".into(),
            Value::String(self.module.as_str().to_string()),
        );
        map.insert("readiness".into(), Value::from(self.readiness));
        map.insert("sla_threshold".into(), Value::from(self.sla_threshold));
        map.insert("breaches".into(), Value::from(self.breaches));
        map.insert("window_start".into(), Value::from(self.window_start));
        map.insert("window_end".into(), Value::from(self.window_end));
        map.insert("issued_at".into(), Value::from(self.issued_at));
        map.insert(
            "signer".into(),
            Value::Array(self.signer.iter().map(|b| Value::from(*b)).collect()),
        );
        map.insert(
            "signature".into(),
            Value::Array(self.signature.iter().map(|b| Value::from(*b)).collect()),
        );
        map.insert(
            "digest".into(),
            Value::Array(self.digest.iter().map(|b| Value::from(*b)).collect()),
        );
        if !self.site_readiness.is_empty() {
            let sites: Vec<Value> = self
                .site_readiness
                .iter()
                .map(|entry| {
                    let mut site_map = Map::new();
                    site_map.insert("site".into(), Value::String(entry.site.clone()));
                    site_map.insert("readiness".into(), Value::from(entry.readiness));
                    Value::Object(site_map)
                })
                .collect();
            map.insert("site_readiness".into(), Value::Array(sites));
        }
        Value::Object(map)
    }

    pub fn from_value(value: Value) -> Result<Self, ChaosAttestationDecodeError> {
        let Value::Object(map) = value else {
            return Err(ChaosAttestationDecodeError::InvalidType("attestation"));
        };

        fn field<'a>(
            map: &'a Map,
            name: &'static str,
        ) -> Result<&'a Value, ChaosAttestationDecodeError> {
            map.get(name)
                .ok_or(ChaosAttestationDecodeError::MissingField(name))
        }

        fn read_bytes<const N: usize>(
            value: &Value,
            field: &'static str,
        ) -> Result<[u8; N], ChaosAttestationDecodeError> {
            let array = value
                .as_array()
                .ok_or(ChaosAttestationDecodeError::InvalidType(field))?;
            if array.len() != N {
                return Err(ChaosAttestationDecodeError::InvalidLength(field));
            }
            let mut bytes = [0u8; N];
            for (idx, entry) in array.iter().enumerate() {
                let value = entry
                    .as_u64()
                    .ok_or(ChaosAttestationDecodeError::InvalidType(field))?;
                if value > u8::MAX as u64 {
                    return Err(ChaosAttestationDecodeError::InvalidType(field));
                }
                bytes[idx] = value as u8;
            }
            Ok(bytes)
        }

        fn read_u64(
            value: &Value,
            field: &'static str,
        ) -> Result<u64, ChaosAttestationDecodeError> {
            value
                .as_u64()
                .ok_or(ChaosAttestationDecodeError::InvalidType(field))
        }

        fn read_f64(
            value: &Value,
            field: &'static str,
        ) -> Result<f64, ChaosAttestationDecodeError> {
            value
                .as_f64()
                .ok_or(ChaosAttestationDecodeError::InvalidType(field))
        }

        let scenario = field(&map, "scenario")?
            .as_str()
            .ok_or(ChaosAttestationDecodeError::InvalidType("scenario"))?
            .to_string();
        let module_value = field(&map, "module")?
            .as_str()
            .ok_or(ChaosAttestationDecodeError::InvalidType("module"))?;
        let module = ChaosModule::from_str(module_value)
            .ok_or_else(|| ChaosAttestationDecodeError::InvalidModule(module_value.to_string()))?;
        let readiness = read_f64(field(&map, "readiness")?, "readiness")?;
        let sla_threshold = read_f64(field(&map, "sla_threshold")?, "sla_threshold")?;
        let breaches = read_u64(field(&map, "breaches")?, "breaches")?;
        let window_start = read_u64(field(&map, "window_start")?, "window_start")?;
        let window_end = read_u64(field(&map, "window_end")?, "window_end")?;
        let issued_at = read_u64(field(&map, "issued_at")?, "issued_at")?;
        let signer = read_bytes::<32>(field(&map, "signer")?, "signer")?;
        let signature = read_bytes::<SIGNATURE_LENGTH>(field(&map, "signature")?, "signature")?;
        let digest = read_bytes::<32>(field(&map, "digest")?, "digest")?;
        let site_readiness = match map.get("site_readiness") {
            Some(Value::Array(entries)) => {
                let mut sites = Vec::with_capacity(entries.len());
                for entry in entries {
                    let Value::Object(site_map) = entry else {
                        return Err(ChaosAttestationDecodeError::InvalidType("site_readiness"));
                    };
                    let site = site_map.get("site").and_then(Value::as_str).ok_or(
                        ChaosAttestationDecodeError::MissingField("site_readiness.site"),
                    )?;
                    let readiness = site_map.get("readiness").and_then(Value::as_f64).ok_or(
                        ChaosAttestationDecodeError::MissingField("site_readiness.readiness"),
                    )?;
                    sites.push(ChaosSiteReadiness {
                        site: site.to_string(),
                        readiness,
                    });
                }
                sites
            }
            Some(_) => return Err(ChaosAttestationDecodeError::InvalidType("site_readiness")),
            None => Vec::new(),
        };

        Ok(ChaosAttestation {
            scenario,
            module,
            readiness,
            sla_threshold,
            breaches,
            window_start,
            window_end,
            issued_at,
            signer,
            signature,
            digest,
            site_readiness,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChaosAttestationError {
    InvalidSignature,
    DigestMismatch,
    InvalidWindow,
    InvalidReadiness,
}

impl fmt::Display for ChaosAttestationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChaosAttestationError::InvalidSignature => {
                write!(f, "invalid chaos attestation signature")
            }
            ChaosAttestationError::DigestMismatch => write!(f, "chaos attestation digest mismatch"),
            ChaosAttestationError::InvalidWindow => {
                write!(f, "chaos attestation window_start must be <= window_end")
            }
            ChaosAttestationError::InvalidReadiness => {
                write!(f, "chaos readiness must be between 0 and 1")
            }
        }
    }
}

impl std::error::Error for ChaosAttestationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChaosAttestationDecodeError {
    MissingField(&'static str),
    InvalidType(&'static str),
    InvalidModule(String),
    InvalidLength(&'static str),
}

impl fmt::Display for ChaosAttestationDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChaosAttestationDecodeError::MissingField(field) => {
                write!(f, "chaos attestation missing field '{field}'")
            }
            ChaosAttestationDecodeError::InvalidType(field) => {
                write!(f, "chaos attestation field '{field}' has invalid type")
            }
            ChaosAttestationDecodeError::InvalidModule(value) => {
                write!(f, "unknown chaos module '{value}'")
            }
            ChaosAttestationDecodeError::InvalidLength(field) => {
                write!(f, "chaos attestation field '{field}' has invalid length")
            }
        }
    }
}

impl std::error::Error for ChaosAttestationDecodeError {}

/// Sign a chaos attestation draft with the provided signing key.
pub fn sign_attestation(draft: ChaosAttestationDraft, key: &SigningKey) -> ChaosAttestation {
    let normalized = draft.normalize();
    let digest = normalized.digest();
    let signature = key.sign(&digest);
    ChaosAttestation {
        scenario: normalized.scenario,
        module: normalized.module,
        readiness: normalized.readiness,
        sla_threshold: normalized.sla_threshold,
        breaches: normalized.breaches,
        window_start: normalized.window_start,
        window_end: normalized.window_end,
        issued_at: normalized.issued_at,
        signer: key.verifying_key().to_bytes(),
        signature: signature.to_bytes(),
        digest,
        site_readiness: normalized.site_readiness,
    }
}

/// Validate a signed chaos attestation.
pub fn verify_attestation(attestation: &ChaosAttestation) -> Result<(), ChaosAttestationError> {
    if attestation.window_start > attestation.window_end {
        return Err(ChaosAttestationError::InvalidWindow);
    }
    if !(0.0..=1.0).contains(&attestation.readiness)
        || !(0.0..=1.0).contains(&attestation.sla_threshold)
    {
        return Err(ChaosAttestationError::InvalidReadiness);
    }
    if attestation
        .site_readiness
        .iter()
        .any(|entry| !(0.0..=1.0).contains(&entry.readiness))
    {
        return Err(ChaosAttestationError::InvalidReadiness);
    }
    let expected = attestation.draft().digest();
    if expected != attestation.digest {
        return Err(ChaosAttestationError::DigestMismatch);
    }
    let verifying = VerifyingKey::from_bytes(&attestation.signer)
        .map_err(|_| ChaosAttestationError::InvalidSignature)?;
    let signature = Signature::from_bytes(&attestation.signature);
    verifying
        .verify(&attestation.digest, &signature)
        .map_err(|_| ChaosAttestationError::InvalidSignature)
}

/// Aggregated readiness snapshot used by dashboards and operator tooling.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChaosReadinessSnapshot {
    pub scenario: String,
    pub module: ChaosModule,
    pub readiness: f64,
    pub sla_threshold: f64,
    pub breaches: u64,
    pub window_start: u64,
    pub window_end: u64,
    pub issued_at: u64,
    pub signer: [u8; 32],
    pub digest: [u8; 32],
    pub site_readiness: Vec<ChaosSiteReadiness>,
}

impl From<&ChaosAttestation> for ChaosReadinessSnapshot {
    fn from(attestation: &ChaosAttestation) -> Self {
        ChaosReadinessSnapshot {
            scenario: attestation.scenario.clone(),
            module: attestation.module,
            readiness: attestation.readiness,
            sla_threshold: attestation.sla_threshold,
            breaches: attestation.breaches,
            window_start: attestation.window_start,
            window_end: attestation.window_end,
            issued_at: attestation.issued_at,
            signer: attestation.signer,
            digest: attestation.digest,
            site_readiness: attestation.site_readiness.clone(),
        }
    }
}

impl ChaosReadinessSnapshot {
    pub fn key(&self) -> (String, ChaosModule) {
        (self.scenario.clone(), self.module)
    }

    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("scenario".into(), Value::String(self.scenario.clone()));
        map.insert(
            "module".into(),
            Value::String(self.module.as_str().to_string()),
        );
        map.insert("readiness".into(), Value::from(self.readiness));
        map.insert("sla_threshold".into(), Value::from(self.sla_threshold));
        map.insert("breaches".into(), Value::from(self.breaches));
        map.insert("window_start".into(), Value::from(self.window_start));
        map.insert("window_end".into(), Value::from(self.window_end));
        map.insert("issued_at".into(), Value::from(self.issued_at));
        map.insert(
            "signer".into(),
            Value::Array(self.signer.iter().map(|b| Value::from(*b)).collect()),
        );
        map.insert(
            "digest".into(),
            Value::Array(self.digest.iter().map(|b| Value::from(*b)).collect()),
        );
        if !self.site_readiness.is_empty() {
            let entries: Vec<Value> = self
                .site_readiness
                .iter()
                .map(|entry| {
                    let mut site = Map::new();
                    site.insert("site".into(), Value::String(entry.site.clone()));
                    site.insert("readiness".into(), Value::from(entry.readiness));
                    Value::Object(site)
                })
                .collect();
            map.insert("site_readiness".into(), Value::Array(entries));
        }
        Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::signatures::ed25519::SigningKey;
    use foundation_serialization::json::Value;

    fn signing_key() -> SigningKey {
        // Deterministic signing key for reproducible tests.
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn normalizes_attestation_bounds() {
        let draft = ChaosAttestationDraft {
            scenario: "test".into(),
            module: ChaosModule::Overlay,
            readiness: 1.2,
            sla_threshold: -0.3,
            breaches: 0,
            window_start: 1,
            window_end: 2,
            issued_at: 3,
            site_readiness: vec![ChaosSiteReadiness {
                site: "primary".into(),
                readiness: 1.5,
            }],
        };
        let attestation = sign_attestation(draft, &signing_key());
        assert!((0.0..=1.0).contains(&attestation.readiness));
        assert!((0.0..=1.0).contains(&attestation.sla_threshold));
        assert_eq!(attestation.site_readiness.len(), 1);
        assert!((0.0..=1.0).contains(&attestation.site_readiness[0].readiness));
    }

    #[test]
    fn rejects_invalid_window_bounds() {
        let draft = ChaosAttestationDraft {
            scenario: "window".into(),
            module: ChaosModule::Compute,
            readiness: 0.5,
            sla_threshold: 0.4,
            breaches: 0,
            window_start: 5,
            window_end: 4,
            issued_at: 5,
            site_readiness: Vec::new(),
        };
        let attestation = sign_attestation(draft, &signing_key());
        assert!(matches!(
            verify_attestation(&attestation),
            Err(ChaosAttestationError::InvalidWindow)
        ));
    }

    #[test]
    fn detects_digest_tampering() {
        let draft = ChaosAttestationDraft {
            scenario: "digest".into(),
            module: ChaosModule::Storage,
            readiness: 0.5,
            sla_threshold: 0.4,
            breaches: 0,
            window_start: 1,
            window_end: 2,
            issued_at: 3,
            site_readiness: Vec::new(),
        };
        let mut attestation = sign_attestation(draft, &signing_key());
        attestation.digest[0] ^= 0xFF;
        assert!(matches!(
            verify_attestation(&attestation),
            Err(ChaosAttestationError::DigestMismatch)
        ));
    }

    #[test]
    fn decode_rejects_unknown_module() {
        let draft = ChaosAttestationDraft {
            scenario: "module".into(),
            module: ChaosModule::Overlay,
            readiness: 0.7,
            sla_threshold: 0.6,
            breaches: 1,
            window_start: 10,
            window_end: 20,
            issued_at: 30,
            site_readiness: Vec::new(),
        };
        let attestation = sign_attestation(draft, &signing_key());
        let mut value = attestation.to_value();
        if let Value::Object(ref mut map) = value {
            map.insert("module".into(), Value::from("mystery"));
        }
        match ChaosAttestation::from_value(value) {
            Err(ChaosAttestationDecodeError::InvalidModule(name)) => {
                assert_eq!(name, "mystery")
            }
            other => panic!("expected invalid module error, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_malformed_signer_array() {
        let draft = ChaosAttestationDraft {
            scenario: "array".into(),
            module: ChaosModule::Compute,
            readiness: 0.9,
            sla_threshold: 0.8,
            breaches: 0,
            window_start: 1,
            window_end: 2,
            issued_at: 3,
            site_readiness: Vec::new(),
        };
        let attestation = sign_attestation(draft, &signing_key());
        let mut value = attestation.to_value();
        if let Value::Object(ref mut map) = value {
            map.insert("signer".into(), Value::Array(vec![Value::from(1)]));
        }
        match ChaosAttestation::from_value(value) {
            Err(ChaosAttestationDecodeError::InvalidLength(field)) => {
                assert_eq!(field, "signer");
            }
            other => panic!("expected invalid signer length, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_malformed_site_readiness() {
        let draft = ChaosAttestationDraft {
            scenario: "site".into(),
            module: ChaosModule::Overlay,
            readiness: 0.8,
            sla_threshold: 0.7,
            breaches: 0,
            window_start: 1,
            window_end: 5,
            issued_at: 6,
            site_readiness: vec![ChaosSiteReadiness {
                site: "east".into(),
                readiness: 0.5,
            }],
        };
        let attestation = sign_attestation(draft, &signing_key());
        let mut value = attestation.to_value();
        if let Value::Object(ref mut map) = value {
            map.insert(
                "site_readiness".into(),
                Value::Array(vec![Value::from("invalid")]),
            );
        }
        match ChaosAttestation::from_value(value) {
            Err(ChaosAttestationDecodeError::InvalidType(field)) => {
                assert_eq!(field, "site_readiness");
            }
            other => panic!("expected invalid site readiness type, got {other:?}"),
        }
    }
}
