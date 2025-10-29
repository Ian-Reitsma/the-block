use crate::{
    SelectionAttestation, SelectionAttestationKind, SelectionCommitteeTranscript,
    SelectionProofMetadata, SelectionReceipt, SelectionReceiptError,
};
use crypto_suite::hashing::blake3;
use foundation_metrics::{gauge, histogram, increment_counter};
use foundation_serialization::{
    self as serialization,
    json::{self, Map as JsonMap, Value as JsonValue},
};
use std::collections::HashSet;
use std::time::Instant;
use zkp::selection::SelectionProofVerification;

#[derive(Clone, Debug)]
pub struct SelectionAttestationConfig {
    pub preferred_circuit_ids: HashSet<String>,
    pub allow_tee_fallback: bool,
    pub require_attestation: bool,
}

impl SelectionAttestationConfig {
    pub fn normalized(mut self) -> Self {
        self.preferred_circuit_ids = self
            .preferred_circuit_ids
            .into_iter()
            .map(|id| id.trim().to_lowercase())
            .filter(|id| !id.is_empty())
            .collect();
        self
    }
}

impl Default for SelectionAttestationConfig {
    fn default() -> Self {
        Self {
            preferred_circuit_ids: HashSet::new(),
            allow_tee_fallback: true,
            require_attestation: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestationSatisfaction {
    Satisfied,
    Missing,
}

#[derive(Clone, Debug)]
pub struct SelectionAttestationManager {
    config: SelectionAttestationConfig,
}

impl SelectionAttestationManager {
    pub fn new(config: SelectionAttestationConfig) -> Self {
        Self {
            config: config.normalized(),
        }
    }

    pub fn config(&self) -> &SelectionAttestationConfig {
        &self.config
    }

    fn normalize_transcripts(
        &self,
        metadata: &SelectionProofMetadata,
        transcripts: &[SelectionCommitteeTranscript],
    ) -> Vec<SelectionCommitteeTranscript> {
        let Some(expected_digest) = metadata.proof_digest_array() else {
            increment_counter!(
                "ad_selection_transcript_total",
                "result" => "rejected",
                "reason" => "metadata"
            );
            return Vec::new();
        };
        let mut accepted = Vec::new();
        for transcript in transcripts {
            if transcript.transcript.is_empty() {
                increment_counter!(
                    "ad_selection_transcript_total",
                    "result" => "rejected",
                    "reason" => "empty"
                );
                continue;
            }
            let mut normalized = transcript.clone().normalized();
            if !normalized.update_metadata(metadata.manifest_epoch, &expected_digest) {
                increment_counter!(
                    "ad_selection_transcript_total",
                    "result" => "rejected",
                    "reason" => "digest"
                );
                continue;
            }
            increment_counter!(
                "ad_selection_transcript_total",
                "result" => "accepted"
            );
            accepted.push(normalized);
        }
        accepted
    }

    pub fn attach_attestation(
        &self,
        receipt: &SelectionReceipt,
        provided: &[SelectionAttestation],
        transcripts: &[SelectionCommitteeTranscript],
    ) -> (
        Option<SelectionAttestation>,
        AttestationSatisfaction,
        Option<SelectionProofMetadata>,
        Vec<SelectionCommitteeTranscript>,
    ) {
        let commitment = match compute_commitment(receipt) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("failed to compute selection commitment for attestation: {err}");
                if self.config.require_attestation {
                    increment_counter!(
                        "ad_selection_attestation_total",
                        "kind" => "missing",
                        "result" => "rejected",
                        "reason" => "commitment"
                    );
                    return (None, AttestationSatisfaction::Missing, None, Vec::new());
                } else {
                    increment_counter!(
                        "ad_selection_attestation_total",
                        "kind" => "missing",
                        "result" => "accepted",
                        "reason" => "commitment"
                    );
                    return (None, AttestationSatisfaction::Satisfied, None, Vec::new());
                }
            }
        };

        let mut tee_candidate: Option<SelectionAttestation> = None;
        for attestation in provided {
            match attestation {
                SelectionAttestation::Snark { proof, circuit_id } => {
                    let circuit_id = circuit_id.trim().to_lowercase();
                    if !self.config.preferred_circuit_ids.is_empty()
                        && !self.config.preferred_circuit_ids.contains(&circuit_id)
                    {
                        increment_counter!(
                            "ad_selection_attestation_total",
                            "kind" => "snark",
                            "result" => "rejected",
                            "reason" => "circuit"
                        );
                        continue;
                    }
                    let started = Instant::now();
                    match zkp::selection::verify_selection_proof(&circuit_id, proof, &commitment) {
                        Ok(verification) => {
                            increment_counter!(
                                "ad_selection_attestation_total",
                                "kind" => "snark",
                                "result" => "accepted"
                            );
                            histogram!(
                                "ad_selection_proof_verify_seconds",
                                started.elapsed().as_secs_f64(),
                                "circuit" => circuit_id.clone()
                            );
                            gauge!(
                                "ad_selection_attestation_commitment_bytes",
                                proof.len() as f64,
                                "kind" => "snark"
                            );
                            let metadata = SelectionProofMetadata::from_verification(
                                circuit_id.clone(),
                                verification,
                            );
                            let normalized_transcripts =
                                self.normalize_transcripts(&metadata, transcripts);
                            return (
                                Some(SelectionAttestation::Snark {
                                    proof: proof.clone(),
                                    circuit_id,
                                }),
                                AttestationSatisfaction::Satisfied,
                                Some(metadata),
                                normalized_transcripts,
                            );
                        }
                        Err(err) => {
                            let reason = SelectionProofError::from(err).as_metric_reason();
                            increment_counter!(
                                "ad_selection_attestation_total",
                                "kind" => "snark",
                                "result" => "rejected",
                                "reason" => reason
                            );
                        }
                    }
                }
                SelectionAttestation::Tee { report, quote } => {
                    if report.is_empty() || quote.is_empty() {
                        increment_counter!(
                            "ad_selection_attestation_total",
                            "kind" => "tee",
                            "result" => "rejected",
                            "reason" => "empty"
                        );
                        continue;
                    }
                    tee_candidate = Some(SelectionAttestation::Tee {
                        report: report.clone(),
                        quote: quote.clone(),
                    });
                }
            }
        }

        if self.config.allow_tee_fallback {
            if let Some(attestation) = tee_candidate {
                increment_counter!(
                    "ad_selection_attestation_total",
                    "kind" => "tee",
                    "result" => "fallback"
                );
                return (
                    Some(attestation),
                    AttestationSatisfaction::Satisfied,
                    None,
                    Vec::new(),
                );
            }
        }

        if self.config.require_attestation {
            increment_counter!(
                "ad_selection_attestation_total",
                "kind" => "missing",
                "result" => "rejected"
            );
            (None, AttestationSatisfaction::Missing, None, Vec::new())
        } else {
            increment_counter!(
                "ad_selection_attestation_total",
                "kind" => "missing",
                "result" => "accepted"
            );
            (None, AttestationSatisfaction::Satisfied, None, Vec::new())
        }
    }

    pub fn validate_receipt(
        &self,
        receipt: &SelectionReceipt,
    ) -> Result<(), SelectionReceiptError> {
        match receipt.attestation_kind() {
            SelectionAttestationKind::Snark => {
                if let Some(SelectionAttestation::Snark { proof, circuit_id }) =
                    &receipt.attestation
                {
                    let metadata = receipt.proof_metadata.as_ref().ok_or(
                        SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "metadata",
                        },
                    )?;
                    let commitment = compute_commitment(receipt).map_err(|_| {
                        SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "commitment",
                        }
                    })?;
                    if !self.config.preferred_circuit_ids.is_empty()
                        && !self
                            .config
                            .preferred_circuit_ids
                            .contains(&circuit_id.to_lowercase())
                    {
                        return Err(SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "circuit",
                        });
                    }
                    let normalized = circuit_id.to_lowercase();
                    if metadata.circuit_id != normalized {
                        return Err(SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "metadata",
                        });
                    }
                    let verification =
                        zkp::selection::verify_selection_proof(&normalized, proof, &commitment)
                            .map_err(|err| {
                                let reason = SelectionProofError::from(err).as_metric_reason();
                                SelectionReceiptError::InvalidAttestation {
                                    kind: SelectionAttestationKind::Snark,
                                    reason,
                                }
                            })?;
                    if verification.revision != metadata.circuit_revision
                        || verification.public_inputs != metadata.public_inputs
                        || metadata
                            .proof_digest_array()
                            .map(|digest| digest != verification.proof_digest)
                            .unwrap_or(true)
                        || (metadata.proof_length != 0
                            && metadata.proof_length != verification.proof_len)
                        || !protocols_consistent(&metadata.protocol, &verification.protocol)
                        || metadata.transcript_domain_separator.is_none()
                        || metadata
                            .transcript_domain_separator
                            .as_ref()
                            .map(|domain| {
                                !domain
                                    .eq_ignore_ascii_case(&verification.transcript_domain_separator)
                            })
                            .unwrap_or(false)
                        || metadata.expected_witness_commitments.is_none()
                        || metadata
                            .expected_witness_commitments()
                            .map(|expected| {
                                Some(expected)
                                    != verification
                                        .expected_witness_commitments
                                        .map(|value| value as usize)
                            })
                            .unwrap_or(false)
                        || !commitments_consistent(&metadata, &verification)
                    {
                        return Err(SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "metadata",
                        });
                    }
                    if !receipt.committee_transcripts.is_empty() {
                        let expected_digest = metadata.proof_digest_array().ok_or(
                            SelectionReceiptError::InvalidTranscript { reason: "metadata" },
                        )?;
                        for transcript in &receipt.committee_transcripts {
                            if transcript.transcript.is_empty() {
                                return Err(SelectionReceiptError::InvalidTranscript {
                                    reason: "empty",
                                });
                            }
                            if transcript
                                .manifest_epoch
                                .filter(|epoch| *epoch == metadata.manifest_epoch)
                                .is_none()
                            {
                                return Err(SelectionReceiptError::InvalidTranscript {
                                    reason: "manifest_epoch",
                                });
                            }
                            if transcript.transcript_digest.is_empty() {
                                let computed = transcript.compute_digest().ok_or(
                                    SelectionReceiptError::InvalidTranscript { reason: "empty" },
                                )?;
                                if computed != expected_digest {
                                    return Err(SelectionReceiptError::InvalidTranscript {
                                        reason: "digest",
                                    });
                                }
                            } else if transcript.transcript_digest.as_slice() != expected_digest {
                                return Err(SelectionReceiptError::InvalidTranscript {
                                    reason: "digest",
                                });
                            }
                        }
                    }
                    Ok(())
                } else {
                    Err(SelectionReceiptError::InvalidAttestation {
                        kind: SelectionAttestationKind::Snark,
                        reason: "missing",
                    })
                }
            }
            SelectionAttestationKind::Tee => {
                if let Some(SelectionAttestation::Tee { report, quote }) = &receipt.attestation {
                    if report.is_empty() || quote.is_empty() {
                        return Err(SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Tee,
                            reason: "empty",
                        });
                    }
                    Ok(())
                } else {
                    Err(SelectionReceiptError::InvalidAttestation {
                        kind: SelectionAttestationKind::Tee,
                        reason: "missing",
                    })
                }
            }
            SelectionAttestationKind::Missing => {
                if self.config.require_attestation {
                    Err(SelectionReceiptError::InvalidAttestation {
                        kind: SelectionAttestationKind::Missing,
                        reason: "required",
                    })
                } else {
                    Ok(())
                }
            }
        }
    }
}

fn protocols_consistent(meta: &Option<String>, verification: &Option<String>) -> bool {
    match (meta.as_ref(), verification.as_ref()) {
        (Some(meta), Some(ver)) => meta.eq_ignore_ascii_case(ver),
        (Some(_), None) => false,
        (None, Some(_)) => true,
        (None, None) => true,
    }
}

fn commitments_consistent(
    metadata: &SelectionProofMetadata,
    verification: &SelectionProofVerification,
) -> bool {
    if verification.witness_commitments.is_empty() {
        return metadata.witness_commitments.is_empty();
    }
    match metadata.witness_commitment_arrays() {
        Some(arrays) => {
            if arrays.is_empty() {
                return true;
            }
            arrays == verification.witness_commitments
        }
        None => false,
    }
}

fn compute_commitment(receipt: &SelectionReceipt) -> Result<[u8; 32], serialization::Error> {
    let mut candidates_values = Vec::with_capacity(receipt.candidates.len());
    for candidate in &receipt.candidates {
        let mut candidate_map = JsonMap::new();
        candidate_map.insert(
            "campaign_id".into(),
            JsonValue::String(candidate.campaign_id.clone()),
        );
        candidate_map.insert(
            "creative_id".into(),
            JsonValue::String(candidate.creative_id.clone()),
        );
        candidate_map.insert(
            "base_bid_usd_micros".into(),
            JsonValue::from(candidate.base_bid_usd_micros),
        );
        candidate_map.insert(
            "quality_adjusted_bid_usd_micros".into(),
            JsonValue::from(candidate.quality_adjusted_bid_usd_micros),
        );
        candidate_map.insert(
            "available_budget_usd_micros".into(),
            JsonValue::from(candidate.available_budget_usd_micros),
        );
        candidate_map.insert(
            "action_rate_ppm".into(),
            JsonValue::from(candidate.action_rate_ppm),
        );
        candidate_map.insert("lift_ppm".into(), JsonValue::from(candidate.lift_ppm));
        candidate_map.insert(
            "quality_multiplier".into(),
            JsonValue::from(candidate.quality_multiplier),
        );
        candidate_map.insert(
            "pacing_kappa".into(),
            JsonValue::from(candidate.pacing_kappa),
        );
        candidates_values.push(JsonValue::Object(candidate_map));
    }
    let mut commitment_map = JsonMap::new();
    commitment_map.insert(
        "domain".into(),
        JsonValue::String(receipt.cohort.domain.clone()),
    );
    commitment_map.insert(
        "provider".into(),
        receipt
            .cohort
            .provider
            .as_ref()
            .map(|value| JsonValue::String(value.clone()))
            .unwrap_or(JsonValue::Null),
    );
    let badge_values = receipt
        .cohort
        .badges
        .iter()
        .cloned()
        .map(JsonValue::String)
        .collect();
    commitment_map.insert("badges".into(), JsonValue::Array(badge_values));
    commitment_map.insert("bytes".into(), JsonValue::from(receipt.cohort.bytes));
    commitment_map.insert(
        "price_per_mib_usd_micros".into(),
        JsonValue::from(receipt.cohort.price_per_mib_usd_micros),
    );
    commitment_map.insert(
        "winner_index".into(),
        JsonValue::from(receipt.winner_index as u64),
    );
    commitment_map.insert(
        "runner_up_quality_bid_usd_micros".into(),
        JsonValue::from(receipt.runner_up_quality_bid_usd_micros),
    );
    commitment_map.insert(
        "clearing_price_usd_micros".into(),
        JsonValue::from(receipt.clearing_price_usd_micros),
    );
    commitment_map.insert(
        "resource_floor_usd_micros".into(),
        JsonValue::from(receipt.resource_floor_usd_micros),
    );
    commitment_map.insert("candidates".into(), JsonValue::Array(candidates_values));
    let serialized = json::to_vec_value(&JsonValue::Object(commitment_map));
    Ok(*blake3::hash(&serialized).as_bytes())
}

pub fn selection_commitment(receipt: &SelectionReceipt) -> Result<[u8; 32], serialization::Error> {
    compute_commitment(receipt)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionProofError {
    Invalid,
    Length,
    Commitment,
    Unsupported,
    Revision,
    Format,
    Semantics,
}

impl SelectionProofError {
    pub fn as_metric_reason(self) -> &'static str {
        match self {
            SelectionProofError::Invalid => "invalid",
            SelectionProofError::Length => "length",
            SelectionProofError::Commitment => "commitment",
            SelectionProofError::Unsupported => "unsupported",
            SelectionProofError::Revision => "revision",
            SelectionProofError::Format => "format",
            SelectionProofError::Semantics => "semantics",
        }
    }
}

impl From<zkp::selection::SelectionProofError> for SelectionProofError {
    fn from(err: zkp::selection::SelectionProofError) -> Self {
        match err {
            zkp::selection::SelectionProofError::Commitment => SelectionProofError::Commitment,
            zkp::selection::SelectionProofError::InvalidProof => SelectionProofError::Invalid,
            zkp::selection::SelectionProofError::Length => SelectionProofError::Length,
            zkp::selection::SelectionProofError::UnsupportedCircuit => {
                SelectionProofError::Unsupported
            }
            zkp::selection::SelectionProofError::RevisionMismatch => SelectionProofError::Revision,
            zkp::selection::SelectionProofError::Format => SelectionProofError::Format,
            zkp::selection::SelectionProofError::Semantics => SelectionProofError::Semantics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestMetricsRecorder;
    use crate::{SelectionCandidateTrace, SelectionCohortTrace};
    use std::collections::HashSet;
    use zkp::selection::{
        selection_circuit_summaries, SelectionCircuitSummary, SelectionProofPublicInputs,
        SelectionProofVerification,
    };

    const CIRCUIT_ID: &str = "selection_argmax_v1";

    fn circuit_summary() -> SelectionCircuitSummary {
        selection_circuit_summaries()
            .into_iter()
            .find(|summary| summary.circuit_id == CIRCUIT_ID)
            .expect("circuit summary")
    }

    fn circuit_revision() -> u16 {
        circuit_summary().revision
    }

    fn transcript_digest(inputs: &SelectionProofPublicInputs) -> [u8; 32] {
        zkp::selection::compute_transcript_digest(CIRCUIT_ID, inputs).expect("digest")
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

    fn build_receipt() -> (SelectionReceipt, Vec<u8>, SelectionProofVerification) {
        let mut receipt = SelectionReceipt {
            cohort: SelectionCohortTrace {
                domain: "example.com".into(),
                provider: Some("wallet".into()),
                badges: vec!["badge-a".into(), "badge-b".into()],
                bytes: 512,
                price_per_mib_usd_micros: 1_500_000,
            },
            candidates: vec![
                SelectionCandidateTrace {
                    campaign_id: "campaign-a".into(),
                    creative_id: "creative-1".into(),
                    base_bid_usd_micros: 2_000_000,
                    quality_adjusted_bid_usd_micros: 2_400_000,
                    available_budget_usd_micros: 5_000_000,
                    action_rate_ppm: 42_000,
                    lift_ppm: 55_000,
                    quality_multiplier: 1.2,
                    pacing_kappa: 0.9,
                },
                SelectionCandidateTrace {
                    campaign_id: "campaign-b".into(),
                    creative_id: "creative-2".into(),
                    base_bid_usd_micros: 1_600_000,
                    quality_adjusted_bid_usd_micros: 1_800_000,
                    available_budget_usd_micros: 3_000_000,
                    action_rate_ppm: 37_000,
                    lift_ppm: 41_000,
                    quality_multiplier: 1.1,
                    pacing_kappa: 0.8,
                },
            ],
            winner_index: 0,
            resource_floor_usd_micros: 1_200_000,
            runner_up_quality_bid_usd_micros: 1_800_000,
            clearing_price_usd_micros: 1_800_000,
            attestation: None,
            proof_metadata: None,
            committee_transcripts: Vec::new(),
        };
        let commitment = compute_commitment(&receipt).expect("commitment");
        let inputs = SelectionProofPublicInputs {
            commitment: commitment.to_vec(),
            winner_index: receipt.winner_index as u16,
            winner_quality_bid_usd_micros: receipt.candidates[0].quality_adjusted_bid_usd_micros,
            runner_up_quality_bid_usd_micros: receipt.runner_up_quality_bid_usd_micros,
            resource_floor_usd_micros: receipt.resource_floor_usd_micros,
            clearing_price_usd_micros: receipt.clearing_price_usd_micros,
            candidate_count: receipt.candidates.len() as u16,
        };
        let transcript = transcript_digest(&inputs);
        let mut proof_bytes = vec![0xCC; 96];
        proof_bytes[..transcript.len()].copy_from_slice(&transcript);
        let witness_commitments = vec![[0x55; 32], [0x99; 32]];
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
        let commitments_json = format!(
            "[{},{}]",
            encode_bytes(&witness_commitments[0]),
            encode_bytes(&witness_commitments[1])
        );
        let protocol = circuit_summary()
            .expected_protocol
            .as_deref()
            .unwrap_or("groth16")
            .to_string();
        let proof = format!(
            "{{\"version\":1,\"circuit_revision\":{},\"public_inputs\":{},\"proof\":{{\"protocol\":\"{}\",\"transcript_digest\":{},\"bytes\":{},\"witness_commitments\":{}}}}}",
            circuit_revision(),
            public_inputs,
            protocol,
            encode_bytes(&transcript),
            encode_bytes(&proof_bytes),
            commitments_json,
        )
        .into_bytes();
        let verification = zkp::selection::verify_selection_proof(CIRCUIT_ID, &proof, &commitment)
            .expect("proof verifies");
        receipt.attestation = Some(SelectionAttestation::Snark {
            proof: proof.clone(),
            circuit_id: CIRCUIT_ID.into(),
        });
        receipt.proof_metadata = Some(SelectionProofMetadata::from_verification(
            CIRCUIT_ID.into(),
            verification.clone(),
        ));
        (receipt, proof, verification)
    }

    #[test]
    fn validate_receipt_accepts_valid_metadata() {
        let mut preferred = HashSet::new();
        preferred.insert(CIRCUIT_ID.into());
        let manager = SelectionAttestationManager::new(SelectionAttestationConfig {
            preferred_circuit_ids: preferred,
            allow_tee_fallback: false,
            require_attestation: true,
        });
        let (receipt, _, verification) = build_receipt();
        let metadata = receipt.proof_metadata.as_ref().expect("metadata present");
        assert_eq!(
            metadata.protocol.as_deref(),
            verification.protocol.as_deref()
        );
        assert_eq!(metadata.proof_length, verification.proof_len);
        assert_eq!(metadata.witness_commitments.len(), 2);
        assert_eq!(
            metadata.transcript_domain_separator.as_deref(),
            Some(verification.transcript_domain_separator.as_str())
        );
        assert_eq!(
            metadata.expected_witness_commitments,
            verification.expected_witness_commitments
        );
        assert!(manager.validate_receipt(&receipt).is_ok());
    }

    #[test]
    fn validate_receipt_rejects_metadata_digest_mismatch() {
        let mut preferred = HashSet::new();
        preferred.insert(CIRCUIT_ID.into());
        let manager = SelectionAttestationManager::new(SelectionAttestationConfig {
            preferred_circuit_ids: preferred,
            allow_tee_fallback: false,
            require_attestation: true,
        });
        let (mut receipt, _, verification) = build_receipt();
        let mut metadata =
            SelectionProofMetadata::from_verification(CIRCUIT_ID.into(), verification);
        metadata.proof_digest[0] ^= 0xFF;
        receipt.proof_metadata = Some(metadata);
        let err = manager
            .validate_receipt(&receipt)
            .expect_err("digest mismatch should fail");
        match err {
            SelectionReceiptError::InvalidAttestation { reason, .. } => {
                assert_eq!(reason, "metadata");
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn validate_receipt_rejects_metadata_commitment_mismatch() {
        let mut preferred = HashSet::new();
        preferred.insert(CIRCUIT_ID.into());
        let manager = SelectionAttestationManager::new(SelectionAttestationConfig {
            preferred_circuit_ids: preferred,
            allow_tee_fallback: false,
            require_attestation: true,
        });
        let (mut receipt, _, verification) = build_receipt();
        let mut metadata =
            SelectionProofMetadata::from_verification(CIRCUIT_ID.into(), verification);
        metadata.witness_commitments[0][0] ^= 0x1;
        receipt.proof_metadata = Some(metadata);
        let err = manager
            .validate_receipt(&receipt)
            .expect_err("commitment mismatch should fail");
        match err {
            SelectionReceiptError::InvalidAttestation { reason, .. } => {
                assert_eq!(reason, "metadata");
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn attach_attestation_produces_transcript_metadata() {
        let mut preferred = HashSet::new();
        preferred.insert(CIRCUIT_ID.into());
        let manager = SelectionAttestationManager::new(SelectionAttestationConfig {
            preferred_circuit_ids: preferred,
            allow_tee_fallback: false,
            require_attestation: true,
        });
        let (receipt, proof_bytes, verification) = build_receipt();
        let transcript = SelectionCommitteeTranscript {
            committee_id: "committee-alpha".into(),
            transcript: verification.proof_digest.to_vec(),
            signature: Vec::new(),
            manifest_epoch: None,
            transcript_digest: verification.proof_digest.to_vec(),
        };
        let (attestation, satisfaction, metadata, transcripts) = manager.attach_attestation(
            &receipt,
            &[SelectionAttestation::Snark {
                proof: proof_bytes,
                circuit_id: CIRCUIT_ID.into(),
            }],
            &[transcript],
        );
        assert!(matches!(satisfaction, AttestationSatisfaction::Satisfied));
        assert!(attestation.is_some());
        let metadata = metadata.expect("metadata present");
        assert_eq!(metadata.circuit_revision, verification.revision);
        assert_eq!(transcripts.len(), 1);
        let attached = &transcripts[0];
        assert_eq!(attached.manifest_epoch, Some(metadata.manifest_epoch));
        assert_eq!(
            attached.transcript_digest.as_slice(),
            metadata.proof_digest.as_slice()
        );
    }

    #[test]
    fn attestation_metrics_recorded_under_load() {
        let Some(recorder) = TestMetricsRecorder::install() else {
            eprintln!("skipping metrics assertion; recorder already installed elsewhere");
            return;
        };
        recorder.reset();
        let mut preferred = HashSet::new();
        preferred.insert(CIRCUIT_ID.into());
        let manager = SelectionAttestationManager::new(SelectionAttestationConfig {
            preferred_circuit_ids: preferred,
            allow_tee_fallback: true,
            require_attestation: true,
        });
        let (receipt, proof, _) = build_receipt();
        let attestation = SelectionAttestation::Snark {
            proof: proof.clone(),
            circuit_id: CIRCUIT_ID.into(),
        };
        for _ in 0..5 {
            let _ = manager.attach_attestation(&receipt, &[attestation.clone()], &[]);
        }
        let counters = recorder.counters();
        assert!(counters.iter().any(|event| {
            event.name == "ad_selection_attestation_total"
                && event
                    .labels
                    .iter()
                    .any(|(k, v)| k == "result" && v == "accepted")
                && event
                    .labels
                    .iter()
                    .any(|(k, v)| k == "kind" && v == "snark")
        }));
        let histograms = recorder.histograms();
        assert!(histograms
            .iter()
            .any(|event| event.name == "ad_selection_proof_verify_seconds"));
        let gauges = recorder.gauges();
        assert!(gauges
            .iter()
            .any(|event| event.name == "ad_selection_attestation_commitment_bytes"));
    }
}
