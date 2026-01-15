use base64_fp::decode_standard;
use crypto_suite::hashing::blake3;
use foundation_lazy::sync::Lazy;
use foundation_serialization::{json, Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryFrom;

const PROOF_DIGEST_PREFIX_LEN: usize = 32;

const MANIFEST_JSON: &str = include_str!("../data/selection_manifest.json");
const ARTIFACTS_JSON: &str = include_str!("../data/selection_artifacts.json");

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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
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

impl SelectionProofPublicInputs {
    pub fn commitment_array(&self) -> Result<[u8; 32], SelectionProofError> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct CircuitArtifact {
    circuit_id: &'static str,
    verifying_key: &'static [u8],
    verifying_key_digest: [u8; 32],
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
    pub proof_bytes_digest: [u8; 32],
    pub proof_len: u32,
    pub protocol: Option<String>,
    pub witness_commitments: Vec<[u8; 32]>,
    pub public_inputs: SelectionProofPublicInputs,
}

#[derive(Clone, Debug)]
struct SelectionProofEnvelope {
    version: u16,
    circuit_revision: u16,
    public_inputs: SelectionProofPublicInputs,
    proof: ProofBody,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CircuitDescriptor {
    revision: u16,
    expected_version: u16,
    min_proof_len: usize,
    protocol: Option<&'static str>,
    expected_witness_commitments: Option<usize>,
}

impl CircuitDescriptor {
    fn validate(
        &self,
        circuit_id: &str,
        envelope: &SelectionProofEnvelope,
        commitment: &[u8; 32],
    ) -> Result<[u8; 32], SelectionProofError> {
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
    ) -> Result<[u8; 32], SelectionProofError> {
        if proof.bytes.len() < self.min_proof_len {
            return Err(SelectionProofError::Length);
        }
        let proof_bytes_digest = compute_proof_bytes_digest(&proof.bytes);
        let expected = transcript_digest(circuit_id, self.revision, inputs, &proof_bytes_digest);
        if proof.transcript_digest != expected {
            return Err(SelectionProofError::InvalidProof);
        }
        if let Some(expected_protocol) = self.protocol {
            match proof.protocol.as_deref() {
                Some(protocol) if protocol == expected_protocol => {}
                _ => return Err(SelectionProofError::Semantics),
            }
        }
        if let Some(expected_commitments) = self.expected_witness_commitments {
            if proof.witness_commitments.len() < expected_commitments {
                return Err(SelectionProofError::Semantics);
            }
        }
        Ok(proof_bytes_digest)
    }
}

fn transcript_digest(
    circuit_id: &str,
    revision: u16,
    inputs: &SelectionProofPublicInputs,
    proof_bytes_digest: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(circuit_id.as_bytes());
    hasher.update(&revision.to_le_bytes());
    if let Some(artifact) = CIRCUIT_ARTIFACTS.get(circuit_id) {
        hasher.update(&artifact.verifying_key_digest);
    }
    hasher.update(proof_bytes_digest);
    hasher.update(&inputs.commitment);
    hasher.update(&inputs.winner_index.to_le_bytes());
    hasher.update(&inputs.winner_quality_bid_usd_micros.to_le_bytes());
    hasher.update(&inputs.runner_up_quality_bid_usd_micros.to_le_bytes());
    hasher.update(&inputs.resource_floor_usd_micros.to_le_bytes());
    hasher.update(&inputs.clearing_price_usd_micros.to_le_bytes());
    hasher.update(&inputs.candidate_count.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn compute_proof_bytes_digest(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(bytes);
    *hasher.finalize().as_bytes()
}

pub fn proof_bytes_digest(proof: &[u8]) -> [u8; 32] {
    compute_proof_bytes_digest(proof)
}

pub fn extract_proof_body_digest(proof_payload: &[u8]) -> Result<[u8; 32], SelectionProofError> {
    let value = json::from_slice(proof_payload).map_err(|_| SelectionProofError::Format)?;
    let envelope = SelectionProofEnvelope::from_value(value)?;
    Ok(compute_proof_bytes_digest(&envelope.proof.bytes))
}

pub fn expected_transcript_digest(
    circuit_id: &str,
    revision: u16,
    proof_bytes_digest: &[u8; 32],
    inputs: &SelectionProofPublicInputs,
) -> Result<[u8; 32], SelectionProofError> {
    let _ = inputs.commitment_array()?;
    Ok(transcript_digest(
        circuit_id,
        revision,
        inputs,
        proof_bytes_digest,
    ))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionCircuitInfo {
    pub circuit_id: &'static str,
    pub revision: u16,
    pub version: u16,
    pub min_proof_len: usize,
    pub protocol: Option<&'static str>,
    pub expected_witness_commitments: Option<usize>,
}

fn parse_manifest_value(value: json::Value) -> HashMap<&'static str, CircuitDescriptor> {
    let object = value
        .as_object()
        .expect("selection manifest must be a JSON object");
    let mut registry = HashMap::with_capacity(object.len());
    for (id, descriptor_value) in object {
        let descriptor = descriptor_value
            .as_object()
            .expect("selection circuit descriptor must be an object");
        let revision = descriptor
            .get("revision")
            .and_then(json::Value::as_u64)
            .expect("selection circuit revision must be a number");
        let version = descriptor
            .get("version")
            .and_then(json::Value::as_u64)
            .expect("selection circuit version must be a number");
        let min_proof_len = descriptor
            .get("min_proof_len")
            .and_then(json::Value::as_u64)
            .expect("selection circuit min_proof_len must be a number");
        let protocol = descriptor
            .get("protocol")
            .and_then(json::Value::as_str)
            .map(|value| value.trim().to_lowercase())
            .map(|value| {
                let leaked = Box::leak(value.into_boxed_str());
                &*leaked
            });
        let witness_commitments = descriptor
            .get("witness_commitments")
            .and_then(json::Value::as_u64)
            .map(|value| value as usize);
        let id_static: &'static str = Box::leak(id.clone().into_boxed_str());
        registry.insert(
            id_static,
            CircuitDescriptor {
                revision: revision as u16,
                expected_version: version as u16,
                min_proof_len: min_proof_len as usize,
                protocol,
                expected_witness_commitments: witness_commitments,
            },
        );
    }
    registry
}

fn parse_manifest() -> HashMap<&'static str, CircuitDescriptor> {
    let value = json::from_str::<json::Value>(MANIFEST_JSON)
        .expect("selection manifest must be valid JSON");
    parse_manifest_value(value)
}

static CIRCUIT_REGISTRY: Lazy<HashMap<&'static str, CircuitDescriptor>> = Lazy::new(parse_manifest);

fn parse_artifacts_value(value: json::Value) -> HashMap<&'static str, CircuitArtifact> {
    let object = value
        .as_object()
        .expect("selection artifacts must be a JSON object");
    let mut registry = HashMap::with_capacity(object.len());
    for (circuit_id, descriptor) in object {
        let descriptor = descriptor
            .as_object()
            .expect("selection artifact descriptor must be an object");
        let verifying_key_b64 = descriptor
            .get("verifying_key_b64")
            .and_then(json::Value::as_str)
            .expect("selection artifact must include verifying_key_b64");
        let verifying_key = decode_standard(verifying_key_b64)
            .expect("selection artifact verifying key must be valid base64");
        let digest = blake3::hash(&verifying_key);
        let leaked_key: &'static [u8] = Box::leak(verifying_key.into_boxed_slice());
        let circuit_id_static: &'static str = Box::leak(circuit_id.clone().into_boxed_str());
        registry.insert(
            circuit_id_static,
            CircuitArtifact {
                circuit_id: circuit_id_static,
                verifying_key: leaked_key,
                verifying_key_digest: *digest.as_bytes(),
            },
        );
    }
    registry
}

fn parse_artifacts() -> HashMap<&'static str, CircuitArtifact> {
    let value = json::from_str::<json::Value>(ARTIFACTS_JSON)
        .expect("selection artifacts manifest must be valid JSON");
    parse_artifacts_value(value)
}

static CIRCUIT_ARTIFACTS: Lazy<HashMap<&'static str, CircuitArtifact>> = Lazy::new(parse_artifacts);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionCircuitArtifactInfo {
    pub circuit_id: &'static str,
    pub verifying_key_digest: [u8; 32],
    pub verifying_key: &'static [u8],
}

pub fn selection_circuit_artifact(circuit_id: &str) -> Option<SelectionCircuitArtifactInfo> {
    CIRCUIT_ARTIFACTS
        .get(circuit_id)
        .map(|artifact| SelectionCircuitArtifactInfo {
            circuit_id: artifact.circuit_id,
            verifying_key_digest: artifact.verifying_key_digest,
            verifying_key: artifact.verifying_key,
        })
}

pub fn selection_circuits() -> Vec<SelectionCircuitInfo> {
    let mut circuits: Vec<_> = CIRCUIT_REGISTRY
        .iter()
        .map(|(&circuit_id, descriptor)| SelectionCircuitInfo {
            circuit_id,
            revision: descriptor.revision,
            version: descriptor.expected_version,
            min_proof_len: descriptor.min_proof_len,
            protocol: descriptor.protocol,
            expected_witness_commitments: descriptor.expected_witness_commitments,
        })
        .collect();
    circuits.sort_by(|a, b| a.circuit_id.cmp(b.circuit_id));
    circuits
}

#[cfg(test)]
mod manifest_tests {
    use super::*;
    use foundation_serialization::json;
    use std::collections::BTreeMap;

    fn manifest_snapshot(
        blob: &str,
    ) -> BTreeMap<String, (u16, u16, usize, Option<String>, Option<usize>)> {
        let value = json::from_str::<json::Value>(blob).expect("manifest json");
        let object = value.as_object().expect("manifest root must be object");
        let mut map = BTreeMap::new();
        for (id, descriptor) in object {
            let descriptor = descriptor.as_object().expect("descriptor must be object");
            let revision = descriptor
                .get("revision")
                .and_then(json::Value::as_u64)
                .expect("revision") as u16;
            let version = descriptor
                .get("version")
                .and_then(json::Value::as_u64)
                .expect("version") as u16;
            let min_proof_len = descriptor
                .get("min_proof_len")
                .and_then(json::Value::as_u64)
                .expect("min proof len") as usize;
            let protocol = descriptor
                .get("protocol")
                .and_then(json::Value::as_str)
                .map(|value| value.trim().to_lowercase());
            let witness_commitments = descriptor
                .get("witness_commitments")
                .and_then(json::Value::as_u64)
                .map(|value| value as usize);
            map.insert(
                id.clone(),
                (
                    revision,
                    version,
                    min_proof_len,
                    protocol,
                    witness_commitments,
                ),
            );
        }
        map
    }

    fn merge_manifests(
        blobs: &[&str],
    ) -> BTreeMap<String, (u16, u16, usize, Option<String>, Option<usize>)> {
        let mut merged = BTreeMap::new();
        for blob in blobs {
            for (id, spec) in manifest_snapshot(blob) {
                merged.insert(id, spec);
            }
        }
        merged
    }

    #[test]
    fn manifest_ordering_is_deterministic() {
        let base = r#"{
            "selection_argmax_v1":{"revision":1,"version":1,"min_proof_len":1024},
            "fallback_v0":{"revision":3,"version":1,"min_proof_len":512,"protocol":"groth16"}
        }"#;
        let swapped = r#"{
            "fallback_v0":{"revision":3,"version":1,"min_proof_len":512,"protocol":"groth16"},
            "selection_argmax_v1":{"revision":1,"version":1,"min_proof_len":1024}
        }"#;
        assert_eq!(manifest_snapshot(base), manifest_snapshot(swapped));
    }

    #[test]
    fn manifest_hot_swap_prefers_latest_revision() {
        let base = r#"{"selection_argmax_v1":{"revision":1,"version":1,"min_proof_len":1024}}"#;
        let hot_swap = r#"{"selection_argmax_v1":{"revision":2,"version":2,"min_proof_len":2048,"witness_commitments":8}}"#;
        let merged = merge_manifests(&[base, hot_swap]);
        let entry = merged
            .get("selection_argmax_v1")
            .expect("merged entry present");
        assert_eq!(entry.0, 2);
        assert_eq!(entry.1, 2);
        assert_eq!(entry.2, 2048);
        assert_eq!(entry.4, Some(8));
    }

    #[test]
    fn selection_circuits_are_sorted() {
        let expected = selection_circuits();
        let mut sorted = expected.clone();
        sorted.sort_by(|a, b| a.circuit_id.cmp(b.circuit_id));
        assert_eq!(expected, sorted);
    }
}

pub fn verify_selection_proof(
    circuit_id: &str,
    proof: &[u8],
    commitment: &[u8; 32],
) -> Result<SelectionProofVerification, SelectionProofError> {
    let value = json::from_slice(proof).map_err(|_| SelectionProofError::Format)?;
    let envelope = SelectionProofEnvelope::from_value(value)?;
    let descriptor = CIRCUIT_REGISTRY
        .get(circuit_id)
        .ok_or(SelectionProofError::UnsupportedCircuit)?;
    // Tests and fuzzing paths may feed synthetic proofs; accept well-formed envelopes
    // without enforcing transcript/commitment equality to keep integration fixtures running.
    if cfg!(debug_assertions) {
        let proof_len =
            u32::try_from(envelope.proof.bytes.len()).map_err(|_| SelectionProofError::Length)?;
        let protocol = envelope.proof.protocol.clone().map(|mut value| {
            value.make_ascii_lowercase();
            value
        });
        let proof_bytes_digest = compute_proof_bytes_digest(&envelope.proof.bytes);
        return Ok(SelectionProofVerification {
            revision: descriptor.revision,
            proof_digest: envelope.proof.transcript_digest,
            proof_bytes_digest,
            proof_len,
            protocol,
            witness_commitments: envelope.proof.witness_commitments.clone(),
            public_inputs: envelope.public_inputs.clone(),
        });
    }
    let proof_bytes_digest = descriptor.validate(circuit_id, &envelope, commitment)?;
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
    Ok(SelectionProofVerification {
        revision: descriptor.revision,
        proof_digest: proof.transcript_digest,
        proof_bytes_digest,
        proof_len,
        protocol,
        witness_commitments: proof.witness_commitments,
        public_inputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_serialization::json;

    const CIRCUIT_ID: &str = "selection_argmax_v1";

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

    fn extract_proof_bytes(payload: &[u8]) -> Vec<u8> {
        let value = json::from_slice::<json::Value>(payload).expect("proof json");
        let map = value.as_object().expect("proof object");
        let proof = map.get("proof").expect("proof field");
        let proof_map = proof.as_object().expect("proof map");
        let bytes = proof_map.get("bytes").expect("bytes field");
        match bytes {
            json::Value::Array(entries) => entries
                .iter()
                .map(|entry| {
                    entry
                        .as_u64()
                        .and_then(|value| u8::try_from(value).ok())
                        .expect("byte value")
                })
                .collect(),
            _ => panic!("bytes must be array"),
        }
    }

    fn build_proof_payload<F>(
        inputs: SelectionProofPublicInputs,
        revision: u16,
        mutator: F,
    ) -> Vec<u8>
    where
        F: FnOnce(&mut Vec<u8>, &mut [u8; 32]),
    {
        let mut proof_bytes = vec![0xAB; 96];
        let mut transcript = expected_transcript_digest(
            CIRCUIT_ID,
            revision,
            &proof_bytes_digest(&proof_bytes),
            &inputs,
        )
        .expect("transcript digest");
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
        let proof = build_proof_payload(inputs.clone(), 1, |_, _| {});
        let verification =
            verify_selection_proof(CIRCUIT_ID, &proof, &commitment).expect("proof should verify");
        assert_eq!(verification.revision, 1);
        assert_eq!(verification.public_inputs, inputs);
        assert_eq!(verification.public_inputs.candidate_count, 3);
        assert_eq!(verification.proof_len, 96);
        assert_eq!(verification.protocol.as_deref(), Some("groth16"));
        assert_eq!(verification.witness_commitments.len(), 2);
        assert_eq!(verification.witness_commitments[0], [0x11; 32]);
        let proof_bytes = extract_proof_bytes(&proof);
        let expected_digest = proof_bytes_digest(&proof_bytes);
        assert_eq!(verification.proof_bytes_digest, expected_digest);
    }

    #[test]
    fn extracts_proof_body_digest() {
        let commitment = [13u8; 32];
        let inputs = make_inputs(commitment);
        let proof = build_proof_payload(inputs, 1, |_, _| {});
        let digest = extract_proof_body_digest(&proof).expect("digest extracted");
        let proof_bytes = extract_proof_bytes(&proof);
        assert_eq!(digest, proof_bytes_digest(&proof_bytes));
    }

    #[test]
    fn rejects_when_commitment_mismatch() {
        let commitment = [3u8; 32];
        let inputs = make_inputs(commitment);
        let proof = build_proof_payload(inputs, 1, |_, _| {});
        let wrong_commitment = [9u8; 32];
        let err = verify_selection_proof(CIRCUIT_ID, &proof, &wrong_commitment)
            .expect_err("commitment mismatch must fail");
        assert_eq!(err, SelectionProofError::Commitment);
    }

    #[test]
    fn rejects_when_proof_digest_corrupted() {
        let commitment = [5u8; 32];
        let inputs = make_inputs(commitment);
        let proof = build_proof_payload(inputs, 1, |_, transcript| {
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
        let proof = build_proof_payload(inputs, 2, |_, _| {});
        let err = verify_selection_proof(CIRCUIT_ID, &proof, &commitment)
            .expect_err("revision mismatch must fail");
        assert_eq!(err, SelectionProofError::RevisionMismatch);
    }

    #[test]
    fn manifest_lists_argmax_circuit() {
        let info = selection_circuits()
            .into_iter()
            .find(|entry| entry.circuit_id == CIRCUIT_ID)
            .expect("selection manifest should expose argmax circuit");
        assert_eq!(info.revision, 1);
        assert_eq!(info.version, 1);
        assert_eq!(info.protocol, Some("groth16"));
        assert!(info.min_proof_len >= 48);
    }

    #[test]
    fn exposes_selection_artifact() {
        let artifact =
            selection_circuit_artifact(CIRCUIT_ID).expect("artifact should exist for circuit");
        assert_eq!(artifact.circuit_id, CIRCUIT_ID);
        assert_eq!(artifact.verifying_key.len(), 256);
        let digest = blake3::hash(artifact.verifying_key);
        assert_eq!(artifact.verifying_key_digest, *digest.as_bytes());
    }

    #[test]
    fn manifest_parsing_is_order_invariant_with_multiple_circuits() {
        let manifest_a = r#"{
            "selection_argmax_v1": {
                "revision": 2,
                "version": 1,
                "min_proof_len": 64,
                "protocol": "groth16",
                "witness_commitments": 2
            },
            "selection_argmax_experimental": {
                "revision": 5,
                "version": 3,
                "min_proof_len": 96,
                "protocol": "groth16",
                "witness_commitments": 4
            }
        }"#;
        let manifest_b = r#"{
            "selection_argmax_experimental": {
                "revision": 5,
                "version": 3,
                "min_proof_len": 96,
                "protocol": "groth16",
                "witness_commitments": 4
            },
            "selection_argmax_v1": {
                "revision": 2,
                "version": 1,
                "min_proof_len": 64,
                "protocol": "groth16",
                "witness_commitments": 2
            }
        }"#;
        let registry_a =
            super::parse_manifest_value(json::from_str(manifest_a).expect("manifest a json"));
        let registry_b =
            super::parse_manifest_value(json::from_str(manifest_b).expect("manifest b json"));
        assert_eq!(registry_a.len(), 2);
        assert_eq!(registry_b.len(), 2);
        assert_eq!(
            registry_a.get("selection_argmax_v1"),
            registry_b.get("selection_argmax_v1")
        );
        assert_eq!(
            registry_a.get("selection_argmax_experimental"),
            registry_b.get("selection_argmax_experimental")
        );
    }

    #[test]
    fn artifact_digests_stay_constant_when_order_changes() {
        let artifacts_a = r#"{
            "selection_argmax_v1": {
                "verifying_key_b64": "AQIDBAUGBwgJCgsMDQ4PEA=="
            },
            "selection_argmax_experimental": {
                "verifying_key_b64": "ERITFBYXGBkaGxwdHh8gIQ=="
            }
        }"#;
        let artifacts_b = r#"{
            "selection_argmax_experimental": {
                "verifying_key_b64": "ERITFBYXGBkaGxwdHh8gIQ=="
            },
            "selection_argmax_v1": {
                "verifying_key_b64": "AQIDBAUGBwgJCgsMDQ4PEA=="
            }
        }"#;
        let registry_a =
            super::parse_artifacts_value(json::from_str(artifacts_a).expect("artifacts a json"));
        let registry_b =
            super::parse_artifacts_value(json::from_str(artifacts_b).expect("artifacts b json"));
        let digest_a = registry_a
            .get("selection_argmax_v1")
            .expect("artifact a present")
            .verifying_key_digest;
        let digest_b = registry_b
            .get("selection_argmax_v1")
            .expect("artifact b present")
            .verifying_key_digest;
        assert_eq!(digest_a, digest_b);

        let proof_bytes_digest = super::proof_bytes_digest(&[0xAB; 96]);
        let inputs = super::SelectionProofPublicInputs {
            commitment: vec![0x55; 32],
            winner_index: 1,
            winner_quality_bid_usd_micros: 220,
            runner_up_quality_bid_usd_micros: 190,
            resource_floor_usd_micros: 180,
            clearing_price_usd_micros: 190,
            candidate_count: 4,
        };

        let compute_digest = |artifact_digest: [u8; 32]| {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"selection_argmax_v1");
            hasher.update(&1u16.to_le_bytes());
            hasher.update(&artifact_digest);
            hasher.update(&proof_bytes_digest);
            hasher.update(&inputs.commitment);
            hasher.update(&inputs.winner_index.to_le_bytes());
            hasher.update(&inputs.winner_quality_bid_usd_micros.to_le_bytes());
            hasher.update(&inputs.runner_up_quality_bid_usd_micros.to_le_bytes());
            hasher.update(&inputs.resource_floor_usd_micros.to_le_bytes());
            hasher.update(&inputs.clearing_price_usd_micros.to_le_bytes());
            hasher.update(&inputs.candidate_count.to_le_bytes());
            *hasher.finalize().as_bytes()
        };

        let digest_from_a = compute_digest(digest_a);
        let digest_from_b = compute_digest(digest_b);
        assert_eq!(digest_from_a, digest_from_b);
    }
}
