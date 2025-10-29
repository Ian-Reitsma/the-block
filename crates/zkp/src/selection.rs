use base64_fp::decode_standard;
use crypto_suite::hashing::blake3;
use foundation_lazy::sync::Lazy;
use foundation_serialization::{json, Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryFrom;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::{Arc, RwLock};

const PROOF_DIGEST_PREFIX_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionProofError {
    InvalidProof,
    Length,
    Commitment,
    UnsupportedCircuit,
    RevisionMismatch,
    Format,
    Semantics,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SelectionProofPublicInputs {
    #[serde(default, with = "foundation_serialization::serde_bytes")]
    pub commitment: Vec<u8>,
    #[serde(default)]
    pub winner_index: u16,
    #[serde(default)]
    pub winner_quality_bid_usd_micros: u64,
    #[serde(default)]
    pub runner_up_quality_bid_usd_micros: u64,
    #[serde(default)]
    pub resource_floor_usd_micros: u64,
    #[serde(default)]
    pub clearing_price_usd_micros: u64,
    #[serde(default)]
    pub candidate_count: u16,
}

impl Default for SelectionProofPublicInputs {
    fn default() -> Self {
        Self {
            commitment: Vec::new(),
            winner_index: 0,
            winner_quality_bid_usd_micros: 0,
            runner_up_quality_bid_usd_micros: 0,
            resource_floor_usd_micros: 0,
            clearing_price_usd_micros: 0,
            candidate_count: 0,
        }
    }
}

impl SelectionProofPublicInputs {
    fn commitment_array(&self) -> Result<[u8; 32], SelectionProofError> {
        if self.commitment.len() != 32 {
            return Err(SelectionProofError::Commitment);
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&self.commitment);
        Ok(bytes)
    }
}

#[derive(Clone, Debug)]
struct ProofBody {
    bytes: Vec<u8>,
    transcript_digest: [u8; 32],
    protocol: Option<String>,
    witness_commitments: Vec<[u8; 32]>,
}

fn parse_u64(value: &json::Value) -> Result<u64, SelectionProofError> {
    match value {
        json::Value::Number(num) => num.as_u64().ok_or(SelectionProofError::Format),
        json::Value::String(text) => text.parse::<u64>().map_err(|_| SelectionProofError::Format),
        _ => Err(SelectionProofError::Format),
    }
}

fn parse_u16(value: &json::Value) -> Result<u16, SelectionProofError> {
    let number = parse_u64(value)?;
    u16::try_from(number).map_err(|_| SelectionProofError::Format)
}

fn parse_bytes(value: &json::Value) -> Result<Vec<u8>, SelectionProofError> {
    match value {
        json::Value::Array(elements) => {
            let mut out = Vec::with_capacity(elements.len());
            for element in elements {
                let byte = parse_u64(element)?;
                if byte > 255 {
                    return Err(SelectionProofError::Format);
                }
                out.push(byte as u8);
            }
            Ok(out)
        }
        json::Value::String(encoded) => {
            decode_standard(encoded).map_err(|_| SelectionProofError::Format)
        }
        _ => Err(SelectionProofError::Format),
    }
}

fn parse_commitments(value: &json::Value) -> Result<Vec<[u8; 32]>, SelectionProofError> {
    let entries = value.as_array().ok_or(SelectionProofError::Format)?;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let bytes = parse_bytes(entry)?;
        if bytes.len() != 32 {
            return Err(SelectionProofError::Format);
        }
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&bytes);
        out.push(buf);
    }
    Ok(out)
}

fn parse_proof_body(value: &json::Value) -> Result<ProofBody, SelectionProofError> {
    match value {
        json::Value::Object(map) => {
            let protocol = map
                .get("protocol")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            let transcript_value = map
                .get("transcript_digest")
                .ok_or(SelectionProofError::Format)?;
            let transcript_bytes = parse_bytes(transcript_value)?;
            if transcript_bytes.len() != 32 {
                return Err(SelectionProofError::Format);
            }
            let mut transcript_digest = [0u8; 32];
            transcript_digest.copy_from_slice(&transcript_bytes);
            let bytes_value = map
                .get("bytes")
                .or_else(|| map.get("proof"))
                .or_else(|| map.get("blob"))
                .ok_or(SelectionProofError::Format)?;
            let bytes = parse_bytes(bytes_value)?;
            let witness_commitments = match map.get("witness_commitments") {
                Some(value) => parse_commitments(value)?,
                None => Vec::new(),
            };
            Ok(ProofBody {
                bytes,
                transcript_digest,
                protocol,
                witness_commitments,
            })
        }
        _ => {
            let bytes = parse_bytes(value)?;
            if bytes.len() < PROOF_DIGEST_PREFIX_LEN {
                return Err(SelectionProofError::Length);
            }
            let mut digest = [0u8; 32];
            digest.copy_from_slice(&bytes[..PROOF_DIGEST_PREFIX_LEN]);
            Ok(ProofBody {
                bytes,
                transcript_digest: digest,
                protocol: None,
                witness_commitments: Vec::new(),
            })
        }
    }
}

fn parse_public_inputs(
    value: &json::Value,
) -> Result<SelectionProofPublicInputs, SelectionProofError> {
    let map = match value {
        json::Value::Object(map) => map,
        _ => return Err(SelectionProofError::Format),
    };
    let commitment = parse_bytes(map.get("commitment").ok_or(SelectionProofError::Format)?)?;
    let winner_index = parse_u16(map.get("winner_index").ok_or(SelectionProofError::Format)?)?;
    let winner_quality_bid_usd_micros = parse_u64(
        map.get("winner_quality_bid_usd_micros")
            .ok_or(SelectionProofError::Format)?,
    )?;
    let runner_up_quality_bid_usd_micros = parse_u64(
        map.get("runner_up_quality_bid_usd_micros")
            .ok_or(SelectionProofError::Format)?,
    )?;
    let resource_floor_usd_micros = parse_u64(
        map.get("resource_floor_usd_micros")
            .ok_or(SelectionProofError::Format)?,
    )?;
    let clearing_price_usd_micros = parse_u64(
        map.get("clearing_price_usd_micros")
            .ok_or(SelectionProofError::Format)?,
    )?;
    let candidate_count = parse_u16(
        map.get("candidate_count")
            .ok_or(SelectionProofError::Format)?,
    )?;
    Ok(SelectionProofPublicInputs {
        commitment,
        winner_index,
        winner_quality_bid_usd_micros,
        runner_up_quality_bid_usd_micros,
        resource_floor_usd_micros,
        clearing_price_usd_micros,
        candidate_count,
    })
}

impl SelectionProofEnvelope {
    fn from_value(value: json::Value) -> Result<Self, SelectionProofError> {
        let map = match value {
            json::Value::Object(map) => map,
            _ => return Err(SelectionProofError::Format),
        };
        let version = match map.get("version") {
            Some(value) => parse_u16(value)?,
            None => 1,
        };
        let circuit_revision = match map.get("circuit_revision") {
            Some(value) => parse_u16(value)?,
            None => 1,
        };
        let public_inputs = parse_public_inputs(
            map.get("public_inputs")
                .ok_or(SelectionProofError::Format)?,
        )?;
        let proof = parse_proof_body(map.get("proof").ok_or(SelectionProofError::Format)?)?;
        Ok(Self {
            version,
            circuit_revision,
            public_inputs,
            proof,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectionProofVerification {
    pub revision: u16,
    pub proof_digest: [u8; 32],
    pub proof_len: u32,
    pub protocol: Option<String>,
    pub witness_commitments: Vec<[u8; 32]>,
    pub transcript_domain_separator: String,
    pub expected_witness_commitments: Option<u16>,
    pub public_inputs: SelectionProofPublicInputs,
}

#[derive(Clone, Debug)]
struct SelectionProofEnvelope {
    version: u16,
    circuit_revision: u16,
    public_inputs: SelectionProofPublicInputs,
    proof: ProofBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionManifestError {
    Format(String),
    Empty,
    DuplicateCircuit(String),
    MissingField {
        circuit: String,
        field: &'static str,
    },
    RevisionRegression {
        circuit: String,
        current: u16,
        new_revision: u16,
    },
    EpochRegression {
        current: u64,
        proposed: u64,
    },
}

impl std::fmt::Display for SelectionManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectionManifestError::Format(msg) => write!(f, "manifest format error: {msg}"),
            SelectionManifestError::Empty => {
                write!(f, "manifest must describe at least one circuit")
            }
            SelectionManifestError::DuplicateCircuit(id) => {
                write!(f, "manifest defines circuit '{id}' more than once")
            }
            SelectionManifestError::MissingField { circuit, field } => {
                write!(
                    f,
                    "manifest entry '{circuit}' missing required field '{field}'"
                )
            }
            SelectionManifestError::RevisionRegression {
                circuit,
                current,
                new_revision,
            } => write!(
                f,
                "manifest entry '{circuit}' regressed revision from {current} to {new_revision}",
            ),
            SelectionManifestError::EpochRegression { current, proposed } => write!(
                f,
                "manifest epoch regression from {current} to {proposed} is not allowed",
            ),
        }
    }
}

impl std::error::Error for SelectionManifestError {}

#[derive(Clone, Debug)]
pub struct SelectionCircuitDescriptor {
    revision: u16,
    expected_version: u16,
    min_proof_len: usize,
    domain_separator: Option<String>,
    expected_witness_commitments: Option<usize>,
    expected_protocol: Option<String>,
}

impl SelectionCircuitDescriptor {
    fn validate(
        &self,
        circuit_id: &str,
        envelope: &SelectionProofEnvelope,
        commitment: &[u8; 32],
    ) -> Result<(), SelectionProofError> {
        if envelope.version != self.expected_version {
            return Err(SelectionProofError::RevisionMismatch);
        }
        if envelope.circuit_revision != self.revision {
            return Err(SelectionProofError::RevisionMismatch);
        }
        let inputs = &envelope.public_inputs;
        let provided_commitment = inputs.commitment_array()?;
        if &provided_commitment != commitment {
            return Err(SelectionProofError::Commitment);
        }
        if inputs.candidate_count == 0 || inputs.winner_index >= inputs.candidate_count {
            return Err(SelectionProofError::Semantics);
        }
        if inputs.winner_quality_bid_usd_micros < inputs.resource_floor_usd_micros {
            return Err(SelectionProofError::Semantics);
        }
        if inputs.winner_quality_bid_usd_micros < inputs.runner_up_quality_bid_usd_micros {
            return Err(SelectionProofError::Semantics);
        }
        let clearing_expected = inputs
            .resource_floor_usd_micros
            .max(inputs.runner_up_quality_bid_usd_micros)
            .min(inputs.winner_quality_bid_usd_micros);
        if clearing_expected != inputs.clearing_price_usd_micros {
            return Err(SelectionProofError::Semantics);
        }
        self.verify_proof_bytes(circuit_id, inputs, &envelope.proof)
    }

    fn verify_proof_bytes(
        &self,
        circuit_id: &str,
        inputs: &SelectionProofPublicInputs,
        proof: &ProofBody,
    ) -> Result<(), SelectionProofError> {
        if proof.bytes.len() < self.min_proof_len {
            return Err(SelectionProofError::Length);
        }
        if let Some(expected) = &self.expected_protocol {
            match proof.protocol.as_ref() {
                Some(protocol) if protocol.eq_ignore_ascii_case(expected) => {}
                _ => return Err(SelectionProofError::Semantics),
            }
        }
        if let Some(expected) = self.expected_witness_commitments {
            if proof.witness_commitments.len() != expected {
                return Err(SelectionProofError::Semantics);
            }
        }
        let expected = transcript_digest(circuit_id, self, inputs);
        if proof.transcript_digest != expected {
            return Err(SelectionProofError::InvalidProof);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionCircuitSummary {
    pub circuit_id: String,
    pub revision: u16,
    pub expected_version: u16,
    pub min_proof_len: usize,
    pub expected_protocol: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SelectionManifestVersion {
    epoch: u64,
}

impl SelectionManifestVersion {
    pub fn epoch(self) -> u64 {
        self.epoch
    }
}

#[derive(Default)]
struct CircuitRegistry {
    descriptors: HashMap<String, Arc<SelectionCircuitDescriptor>>,
    manifest_epoch: u64,
    manifest_tag: Option<String>,
}

struct ManifestParseResult {
    descriptors: HashMap<String, SelectionCircuitDescriptor>,
    epoch: Option<u64>,
    tag: Option<String>,
}

impl CircuitRegistry {
    fn descriptor(&self, circuit_id: &str) -> Option<Arc<SelectionCircuitDescriptor>> {
        self.descriptors.get(circuit_id).cloned()
    }

    fn summaries(&self) -> Vec<SelectionCircuitSummary> {
        let mut entries: Vec<_> = self
            .descriptors
            .iter()
            .map(|(id, descriptor)| SelectionCircuitSummary {
                circuit_id: id.clone(),
                revision: descriptor.revision,
                expected_version: descriptor.expected_version,
                min_proof_len: descriptor.min_proof_len,
                expected_protocol: descriptor.expected_protocol.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.circuit_id.cmp(&b.circuit_id));
        entries
    }

    fn manifest_version(&self) -> SelectionManifestVersion {
        SelectionManifestVersion {
            epoch: self.manifest_epoch,
        }
    }

    fn install_manifest(
        &mut self,
        result: ManifestParseResult,
    ) -> Result<SelectionManifestVersion, SelectionManifestError> {
        if result.descriptors.is_empty() {
            return Err(SelectionManifestError::Empty);
        }
        for (id, descriptor) in &result.descriptors {
            if let Some(existing) = self.descriptors.get(id) {
                if descriptor.revision < existing.revision {
                    return Err(SelectionManifestError::RevisionRegression {
                        circuit: id.clone(),
                        current: existing.revision,
                        new_revision: descriptor.revision,
                    });
                }
            }
        }
        let next_epoch = match result.epoch {
            Some(epoch) if epoch < self.manifest_epoch => {
                return Err(SelectionManifestError::EpochRegression {
                    current: self.manifest_epoch,
                    proposed: epoch,
                });
            }
            Some(epoch) => epoch,
            None => self.manifest_epoch.saturating_add(1),
        };
        self.manifest_epoch = next_epoch;
        self.manifest_tag = result.tag;
        self.descriptors = result
            .descriptors
            .into_iter()
            .map(|(id, descriptor)| (id, Arc::new(descriptor)))
            .collect();
        Ok(self.manifest_version())
    }
}

fn parse_manifest_entry(
    circuit_id: &str,
    value: &json::Map,
) -> Result<SelectionCircuitDescriptor, SelectionManifestError> {
    let read_u16 = |field: &'static str| -> Result<u16, SelectionManifestError> {
        value
            .get(field)
            .and_then(|val| val.as_u64())
            .map(|num| num as u16)
            .ok_or_else(|| SelectionManifestError::MissingField {
                circuit: circuit_id.to_owned(),
                field,
            })
    };
    let read_min_proof = || -> Result<usize, SelectionManifestError> {
        Ok(value
            .get("min_proof_len")
            .and_then(|val| val.as_u64())
            .map(|num| num as usize)
            .unwrap_or(PROOF_DIGEST_PREFIX_LEN)
            .max(PROOF_DIGEST_PREFIX_LEN))
    };
    let revision = read_u16("revision")?;
    let expected_version = value
        .get("expected_version")
        .and_then(|val| val.as_u64())
        .map(|num| num as u16)
        .unwrap_or(1);
    let min_proof_len = read_min_proof()?;
    let transcript_domain_separator = value
        .get("transcript_domain_separator")
        .and_then(|val| val.as_str())
        .map(|s| s.to_owned());
    let expected_witness_commitments = value
        .get("expected_witness_commitments")
        .and_then(|val| val.as_u64())
        .map(|num| num as usize);
    let expected_protocol = value
        .get("expected_protocol")
        .and_then(|val| val.as_str())
        .map(|proto| {
            let mut owned = proto.to_owned();
            owned.make_ascii_lowercase();
            owned
        });
    Ok(SelectionCircuitDescriptor {
        revision,
        expected_version,
        min_proof_len,
        domain_separator: transcript_domain_separator,
        expected_witness_commitments,
        expected_protocol,
    })
}

fn parse_manifest_bytes(bytes: &[u8]) -> Result<ManifestParseResult, SelectionManifestError> {
    let value: json::Value =
        json::from_slice(bytes).map_err(|err| SelectionManifestError::Format(err.to_string()))?;
    let map = value
        .as_object()
        .ok_or_else(|| SelectionManifestError::Format("manifest root must be an object".into()))?;
    let mut descriptors = HashMap::new();
    let mut epoch = None;
    let mut tag = None;
    for (key, entry) in map {
        if key.starts_with('_') {
            if key == "_meta" {
                if let Some(meta) = entry.as_object() {
                    if let Some(declared_epoch) = meta.get("epoch").and_then(|val| val.as_u64()) {
                        epoch = Some(declared_epoch);
                    }
                    if let Some(label) = meta.get("tag").and_then(|val| val.as_str()) {
                        tag = Some(label.to_owned());
                    }
                }
            }
            continue;
        }
        if descriptors.contains_key(key) {
            return Err(SelectionManifestError::DuplicateCircuit(key.clone()));
        }
        let entry_map = entry.as_object().ok_or_else(|| {
            SelectionManifestError::Format(format!("manifest entry '{key}' must be a JSON object"))
        })?;
        let descriptor = parse_manifest_entry(key, entry_map)?;
        descriptors.insert(key.clone(), descriptor);
    }
    Ok(ManifestParseResult {
        descriptors,
        epoch,
        tag,
    })
}

fn transcript_digest(
    circuit_id: &str,
    descriptor: &SelectionCircuitDescriptor,
    inputs: &SelectionProofPublicInputs,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    if let Some(domain) = &descriptor.domain_separator {
        hasher.update(domain.as_bytes());
    } else {
        hasher.update(circuit_id.as_bytes());
    }
    hasher.update(&descriptor.revision.to_le_bytes());
    hasher.update(&inputs.commitment);
    hasher.update(&inputs.winner_index.to_le_bytes());
    hasher.update(&inputs.winner_quality_bid_usd_micros.to_le_bytes());
    hasher.update(&inputs.runner_up_quality_bid_usd_micros.to_le_bytes());
    hasher.update(&inputs.resource_floor_usd_micros.to_le_bytes());
    hasher.update(&inputs.clearing_price_usd_micros.to_le_bytes());
    hasher.update(&inputs.candidate_count.to_le_bytes());
    *hasher.finalize().as_bytes()
}

pub fn compute_transcript_digest(
    circuit_id: &str,
    inputs: &SelectionProofPublicInputs,
) -> Result<[u8; 32], SelectionProofError> {
    let descriptor = descriptor_for(circuit_id).ok_or(SelectionProofError::UnsupportedCircuit)?;
    Ok(transcript_digest(circuit_id, descriptor.as_ref(), inputs))
}

static CIRCUIT_REGISTRY: Lazy<RwLock<CircuitRegistry>> = Lazy::new(|| {
    let mut registry = CircuitRegistry::default();
    let embedded = parse_manifest_bytes(include_bytes!("../resources/selection_manifest.json"))
        .expect("embedded selection manifest must parse");
    registry
        .install_manifest(embedded)
        .expect("embedded selection manifest must be valid");
    RwLock::new(registry)
});

pub fn install_selection_manifest(
    bytes: &[u8],
) -> Result<SelectionManifestVersion, SelectionManifestError> {
    let result = parse_manifest_bytes(bytes)?;
    let mut guard = CIRCUIT_REGISTRY
        .write()
        .map_err(|_| SelectionManifestError::Format("manifest registry poisoned".into()))?;
    guard.install_manifest(result)
}

pub fn selection_manifest_version() -> SelectionManifestVersion {
    CIRCUIT_REGISTRY
        .read()
        .map(|guard| guard.manifest_version())
        .unwrap_or_default()
}

pub fn selection_manifest_tag() -> Option<String> {
    CIRCUIT_REGISTRY
        .read()
        .ok()
        .and_then(|guard| guard.manifest_tag.clone())
}

pub fn selection_circuit_summaries() -> Vec<SelectionCircuitSummary> {
    CIRCUIT_REGISTRY
        .read()
        .map(|guard| guard.summaries())
        .unwrap_or_default()
}

fn descriptor_for(circuit_id: &str) -> Option<Arc<SelectionCircuitDescriptor>> {
    CIRCUIT_REGISTRY
        .read()
        .ok()
        .and_then(|guard| guard.descriptor(circuit_id))
}

pub fn verify_selection_proof(
    circuit_id: &str,
    proof: &[u8],
    commitment: &[u8; 32],
) -> Result<SelectionProofVerification, SelectionProofError> {
    let value = json::from_slice(proof).map_err(|_| SelectionProofError::Format)?;
    let envelope = SelectionProofEnvelope::from_value(value)?;
    let descriptor = descriptor_for(circuit_id).ok_or(SelectionProofError::UnsupportedCircuit)?;
    descriptor.validate(circuit_id, &envelope, commitment)?;
    let SelectionProofEnvelope {
        public_inputs,
        proof,
        ..
    } = envelope;
    let proof_len = u32::try_from(proof.bytes.len()).map_err(|_| SelectionProofError::Length)?;
    let protocol = proof.protocol.clone().map(|mut value| {
        value.make_ascii_lowercase();
        value
    });
    let descriptor_ref = descriptor.as_ref();
    let expected_commitments = descriptor_ref
        .expected_witness_commitments
        .map(|value| u16::try_from(value).map_err(|_| SelectionProofError::Semantics))
        .transpose()?;
    let domain_separator = descriptor_ref
        .domain_separator
        .clone()
        .unwrap_or_else(|| circuit_id.to_string());
    Ok(SelectionProofVerification {
        revision: descriptor_ref.revision,
        proof_digest: proof.transcript_digest,
        proof_len,
        protocol,
        witness_commitments: proof.witness_commitments,
        transcript_domain_separator: domain_separator,
        expected_witness_commitments: expected_commitments,
        public_inputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CIRCUIT_ID: &str = "selection_argmax_v1";

    fn circuit_descriptor() -> Arc<SelectionCircuitDescriptor> {
        descriptor_for(CIRCUIT_ID).expect("descriptor")
    }

    fn current_revision() -> u16 {
        circuit_descriptor().revision
    }

    static MANIFEST_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    fn make_inputs(commitment: [u8; 32]) -> SelectionProofPublicInputs {
        SelectionProofPublicInputs {
            commitment: commitment.to_vec(),
            winner_index: 1,
            winner_quality_bid_usd_micros: 2_000_000,
            runner_up_quality_bid_usd_micros: 1_200_000,
            resource_floor_usd_micros: 900_000,
            clearing_price_usd_micros: 1_200_000,
            candidate_count: 3,
        }
    }

    fn encode_bytes(bytes: &[u8]) -> String {
        let mut encoded = String::from("[");
        for (idx, byte) in bytes.iter().enumerate() {
            if idx > 0 {
                encoded.push(',');
            }
            use std::fmt::Write;
            write!(&mut encoded, "{}", byte).expect("write byte");
        }
        encoded.push(']');
        encoded
    }

    fn build_proof_payload<F>(
        inputs: SelectionProofPublicInputs,
        revision: u16,
        mutator: F,
    ) -> Vec<u8>
    where
        F: FnOnce(&mut Vec<u8>, &mut [u8; 32]),
    {
        let mut descriptor = (*descriptor_for(CIRCUIT_ID).expect("descriptor")).clone();
        descriptor.revision = revision;
        let mut transcript = transcript_digest(CIRCUIT_ID, &descriptor, &inputs);
        let mut proof_bytes = vec![0xAB; 96];
        proof_bytes[..transcript.len()].copy_from_slice(&transcript);
        mutator(&mut proof_bytes, &mut transcript);
        let public_inputs = format!(
            "{{\"commitment\":{},\"winner_index\":{},\"winner_quality_bid_usd_micros\":{},\"runner_up_quality_bid_usd_micros\":{},\"resource_floor_usd_micros\":{},\"clearing_price_usd_micros\":{},\"candidate_count\":{}}}",
            encode_bytes(&inputs.commitment),
            inputs.winner_index,
            inputs.winner_quality_bid_usd_micros,
            inputs.runner_up_quality_bid_usd_micros,
            inputs.resource_floor_usd_micros,
            inputs.clearing_price_usd_micros,
            inputs.candidate_count,
        );
        let commitments = format!(
            "[{},{}]",
            encode_bytes(&[0x11; 32]),
            encode_bytes(&[0x22; 32])
        );
        let payload = format!(
            "{{\"version\":1,\"circuit_revision\":{},\"public_inputs\":{},\"proof\":{{\"protocol\":\"groth16\",\"transcript_digest\":{},\"bytes\":{},\"witness_commitments\":{}}}}}",
            revision,
            public_inputs,
            encode_bytes(&transcript),
            encode_bytes(&proof_bytes),
            commitments,
        );
        payload.into_bytes()
    }

    #[test]
    fn verifies_valid_selection_proof() {
        let commitment = [7u8; 32];
        let inputs = make_inputs(commitment);
        let revision = current_revision();
        let proof = build_proof_payload(inputs.clone(), revision, |_, _| {});
        let verification =
            verify_selection_proof(CIRCUIT_ID, &proof, &commitment).expect("proof should verify");
        assert_eq!(verification.revision, revision);
        assert_eq!(verification.public_inputs, inputs);
        assert_eq!(verification.public_inputs.candidate_count, 3);
        assert_eq!(verification.proof_len, 96);
        assert_eq!(verification.protocol.as_deref(), Some("groth16"));
        assert_eq!(verification.witness_commitments.len(), 2);
        assert_eq!(verification.witness_commitments[0], [0x11; 32]);
    }

    #[test]
    fn rejects_when_commitment_mismatch() {
        let commitment = [3u8; 32];
        let inputs = make_inputs(commitment);
        let proof = build_proof_payload(inputs, current_revision(), |_, _| {});
        let wrong_commitment = [9u8; 32];
        let err = verify_selection_proof(CIRCUIT_ID, &proof, &wrong_commitment)
            .expect_err("commitment mismatch must fail");
        assert_eq!(err, SelectionProofError::Commitment);
    }

    #[test]
    fn rejects_when_proof_digest_corrupted() {
        let commitment = [5u8; 32];
        let inputs = make_inputs(commitment);
        let proof = build_proof_payload(inputs, current_revision(), |_, transcript| {
            transcript[0] ^= 0xFF;
        });
        let err = verify_selection_proof(CIRCUIT_ID, &proof, &commitment)
            .expect_err("corrupted digest must fail");
        assert_eq!(err, SelectionProofError::InvalidProof);
    }

    #[test]
    fn rejects_on_revision_mismatch() {
        let commitment = [11u8; 32];
        let inputs = make_inputs(commitment);
        let proof = build_proof_payload(inputs, current_revision().saturating_add(1), |_, _| {});
        let err = verify_selection_proof(CIRCUIT_ID, &proof, &commitment)
            .expect_err("revision mismatch must fail");
        assert_eq!(err, SelectionProofError::RevisionMismatch);
    }

    #[test]
    fn installs_runtime_manifest_updates() {
        let _guard = MANIFEST_TEST_LOCK.lock().unwrap();
        let base_version = selection_manifest_version();
        let descriptor = circuit_descriptor();
        let mut entry = json::Map::new();
        let new_revision = descriptor.revision.saturating_add(1);
        entry.insert(
            "revision".into(),
            json::Value::Number(json::Number::from(new_revision)),
        );
        entry.insert(
            "expected_version".into(),
            json::Value::Number(json::Number::from(descriptor.expected_version)),
        );
        entry.insert(
            "min_proof_len".into(),
            json::Value::Number(json::Number::from(descriptor.min_proof_len as u64)),
        );
        if let Some(domain) = &descriptor.domain_separator {
            entry.insert(
                "transcript_domain_separator".into(),
                json::Value::String(domain.clone()),
            );
        }
        if let Some(commitments) = descriptor.expected_witness_commitments {
            entry.insert(
                "expected_witness_commitments".into(),
                json::Value::Number(json::Number::from(commitments as u64)),
            );
        }
        if let Some(protocol) = &descriptor.expected_protocol {
            entry.insert(
                "expected_protocol".into(),
                json::Value::String(protocol.clone()),
            );
        }
        let mut root = json::Map::new();
        let mut meta = json::Map::new();
        let new_epoch = base_version.epoch().saturating_add(5);
        meta.insert(
            "epoch".into(),
            json::Value::Number(json::Number::from(new_epoch)),
        );
        meta.insert("tag".into(), json::Value::String("test-revision".into()));
        root.insert("_meta".into(), json::Value::Object(meta));
        root.insert(CIRCUIT_ID.into(), json::Value::Object(entry));
        let manifest_bytes = json::to_vec(&json::Value::Object(root)).expect("manifest encode");
        let version = install_selection_manifest(&manifest_bytes).expect("manifest installs");
        assert_eq!(version.epoch(), new_epoch);
        let summaries = selection_circuit_summaries();
        let summary = summaries
            .iter()
            .find(|entry| entry.circuit_id == CIRCUIT_ID)
            .expect("summary present");
        assert_eq!(summary.revision, new_revision);
    }

    #[test]
    fn rejects_manifest_epoch_regression() {
        let _guard = MANIFEST_TEST_LOCK.lock().unwrap();
        let revision = current_revision();
        let manifest = format!(
            "{{\"_meta\":{{\"epoch\":0}},\"selection_argmax_v1\":{{\"revision\":{revision},\"expected_version\":1,\"min_proof_len\":96}}}}"
        );
        let err =
            install_selection_manifest(manifest.as_bytes()).expect_err("epoch regression fails");
        assert!(matches!(
            err,
            SelectionManifestError::EpochRegression { .. }
        ));
    }
}
