use base64_fp::decode_standard;
use crypto_suite::hashing::blake3;
use foundation_lazy::sync::Lazy;
use foundation_serialization::{json, Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryFrom;

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
    pub public_inputs: SelectionProofPublicInputs,
}

#[derive(Clone, Debug)]
struct SelectionProofEnvelope {
    version: u16,
    circuit_revision: u16,
    public_inputs: SelectionProofPublicInputs,
    proof: ProofBody,
}

struct CircuitDescriptor {
    revision: u16,
    expected_version: u16,
    min_proof_len: usize,
}

impl CircuitDescriptor {
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
        let expected = transcript_digest(circuit_id, self.revision, inputs);
        if proof.transcript_digest != expected {
            return Err(SelectionProofError::InvalidProof);
        }
        Ok(())
    }
}

fn transcript_digest(
    circuit_id: &str,
    revision: u16,
    inputs: &SelectionProofPublicInputs,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(circuit_id.as_bytes());
    hasher.update(&revision.to_le_bytes());
    hasher.update(&inputs.commitment);
    hasher.update(&inputs.winner_index.to_le_bytes());
    hasher.update(&inputs.winner_quality_bid_usd_micros.to_le_bytes());
    hasher.update(&inputs.runner_up_quality_bid_usd_micros.to_le_bytes());
    hasher.update(&inputs.resource_floor_usd_micros.to_le_bytes());
    hasher.update(&inputs.clearing_price_usd_micros.to_le_bytes());
    hasher.update(&inputs.candidate_count.to_le_bytes());
    *hasher.finalize().as_bytes()
}

static CIRCUIT_REGISTRY: Lazy<HashMap<&'static str, CircuitDescriptor>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "selection_argmax_v1",
        CircuitDescriptor {
            revision: 1,
            expected_version: 1,
            min_proof_len: 48,
        },
    );
    map
});

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
    Ok(SelectionProofVerification {
        revision: descriptor.revision,
        proof_digest: proof.transcript_digest,
        proof_len,
        protocol,
        witness_commitments: proof.witness_commitments,
        public_inputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn build_proof_payload<F>(
        inputs: SelectionProofPublicInputs,
        revision: u16,
        mutator: F,
    ) -> Vec<u8>
    where
        F: FnOnce(&mut Vec<u8>, &mut [u8; 32]),
    {
        let mut transcript = transcript_digest(CIRCUIT_ID, revision, &inputs);
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
}
