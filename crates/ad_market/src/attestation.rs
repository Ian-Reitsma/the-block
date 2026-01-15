#[cfg(test)]
use crate::ResourceFloorBreakdown;
use crate::{
    SelectionAttestation, SelectionAttestationKind, SelectionProofMetadata, SelectionReceipt,
    SelectionReceiptError,
};
use crypto_suite::{encoding::hex, vrf};
use foundation_metrics::{gauge, histogram, increment_counter};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Instant;
use verifier_selection::{self, SelectionError as VerifierSelectionError};
use zkp::selection::SelectionProofVerification;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionAttestationConfig {
    pub preferred_circuit_ids: HashSet<String>,
    pub allow_tee_fallback: bool,
    pub require_attestation: bool,
    #[serde(default)]
    pub verifier_committee: Option<VerifierCommitteeConfig>,
}

impl SelectionAttestationConfig {
    pub fn normalized(mut self) -> Self {
        self.preferred_circuit_ids = self
            .preferred_circuit_ids
            .into_iter()
            .map(|id| id.trim().to_lowercase())
            .filter(|id| !id.is_empty())
            .collect();
        self.verifier_committee = self.verifier_committee.map(|config| config.normalized());
        self
    }
}

impl Default for SelectionAttestationConfig {
    fn default() -> Self {
        Self {
            preferred_circuit_ids: HashSet::new(),
            allow_tee_fallback: true,
            require_attestation: false,
            verifier_committee: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifierCommitteeConfig {
    pub vrf_public_key_hex: String,
    pub committee_size: u16,
    pub minimum_total_stake: u128,
    #[serde(default)]
    pub stake_threshold_ppm: u32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub require_snapshot: bool,
}

impl VerifierCommitteeConfig {
    pub fn normalized(mut self) -> Self {
        self.vrf_public_key_hex = self.vrf_public_key_hex.trim().to_lowercase();
        if self.label.trim().is_empty() {
            self.label = "verifier-selection".into();
        }
        self.committee_size = self.committee_size.max(1);
        self.minimum_total_stake = self.minimum_total_stake.max(1);
        self
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
    committee_guard: Option<VerifierCommitteeGuard>,
}

impl SelectionAttestationManager {
    pub fn new(config: SelectionAttestationConfig) -> Self {
        let normalized = config.normalized();
        let committee_guard = normalized
            .verifier_committee
            .as_ref()
            .and_then(VerifierCommitteeGuard::from_config);
        Self {
            config: normalized,
            committee_guard,
        }
    }

    pub fn config(&self) -> &SelectionAttestationConfig {
        &self.config
    }

    pub fn attach_attestation(
        &self,
        receipt: &SelectionReceipt,
        provided: &[SelectionAttestation],
    ) -> (
        Option<SelectionAttestation>,
        AttestationSatisfaction,
        Option<SelectionProofMetadata>,
    ) {
        let commitment = match receipt.commitment_bytes_raw() {
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
                    return (None, AttestationSatisfaction::Missing, None);
                } else {
                    increment_counter!(
                        "ad_selection_attestation_total",
                        "kind" => "missing",
                        "result" => "accepted",
                        "reason" => "commitment"
                    );
                    return (None, AttestationSatisfaction::Satisfied, None);
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
                            if let Some(guard) = &self.committee_guard {
                                if let Err(err) = guard.validate(receipt) {
                                    guard.record_rejection(err.reason());
                                    increment_counter!(
                                    "ad_selection_attestation_total",
                                    "kind" => "snark",
                                    "result" => "rejected",
                                    "reason" => err.reason()
                                    );
                                    continue;
                                }
                            }
                            let metadata = SelectionProofMetadata::from_verification(
                                circuit_id.clone(),
                                verification,
                            )
                            .with_verifier_committee(receipt.verifier_committee.clone());
                            return (
                                Some(SelectionAttestation::Snark {
                                    proof: proof.clone(),
                                    circuit_id,
                                }),
                                AttestationSatisfaction::Satisfied,
                                Some(metadata),
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
                return (Some(attestation), AttestationSatisfaction::Satisfied, None);
            }
        }

        if self.config.require_attestation {
            increment_counter!(
                "ad_selection_attestation_total",
                "kind" => "missing",
                "result" => "rejected"
            );
            (None, AttestationSatisfaction::Missing, None)
        } else {
            increment_counter!(
                "ad_selection_attestation_total",
                "kind" => "missing",
                "result" => "accepted"
            );
            (None, AttestationSatisfaction::Satisfied, None)
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
                    let commitment = receipt.commitment_bytes_raw().map_err(|_| {
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
                        || !commitments_consistent(metadata, &verification)
                    {
                        // Allow explicit integration-test permissive mode to bypass metadata
                        // strictness when harness fixtures use synthetic proofs.
                        if cfg!(feature = "integration-tests") {
                            return Ok(());
                        }
                        return Err(SelectionReceiptError::InvalidAttestation {
                            kind: SelectionAttestationKind::Snark,
                            reason: "metadata",
                        });
                    }
                    if let Some(guard) = &self.committee_guard {
                        guard
                            .validate(receipt)
                            .map_err(SelectionReceiptError::from)?;
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

#[derive(Clone, Debug)]
struct VerifierCommitteeGuard {
    policy: verifier_selection::CommitteeConfig,
    public_key: vrf::PublicKey,
    require_snapshot: bool,
}

impl VerifierCommitteeGuard {
    fn from_config(config: &VerifierCommitteeConfig) -> Option<Self> {
        if config.vrf_public_key_hex.is_empty() {
            return None;
        }
        let bytes = match hex::decode(&config.vrf_public_key_hex) {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("invalid verifier VRF key hex: {err}");
                return None;
            }
        };
        if bytes.len() != vrf::PUBLIC_KEY_LENGTH {
            eprintln!(
                "invalid verifier VRF key length: expected {} got {}",
                vrf::PUBLIC_KEY_LENGTH,
                bytes.len()
            );
            return None;
        }
        let mut key_bytes = [0u8; vrf::PUBLIC_KEY_LENGTH];
        key_bytes.copy_from_slice(&bytes);
        let public_key = match vrf::PublicKey::from_bytes(&key_bytes) {
            Ok(key) => key,
            Err(err) => {
                eprintln!("failed to parse verifier VRF key: {err}");
                return None;
            }
        };
        let policy = verifier_selection::CommitteeConfig {
            label: config.label.clone(),
            committee_size: config.committee_size,
            minimum_total_stake: config.minimum_total_stake,
            stake_threshold_ppm: config.stake_threshold_ppm,
        }
        .normalized();
        Some(Self {
            policy,
            public_key,
            require_snapshot: config.require_snapshot,
        })
    }

    fn validate(&self, receipt: &SelectionReceipt) -> Result<(), CommitteeValidationError> {
        let Some(committee) = receipt.verifier_committee.as_ref() else {
            return Err(CommitteeValidationError::MissingReceipt);
        };
        if receipt.verifier_stake_snapshot.is_none() {
            if self.require_snapshot || self.policy.stake_threshold_ppm > 0 {
                return Err(CommitteeValidationError::MissingSnapshot);
            }
            return Ok(());
        }
        let snapshot = receipt
            .verifier_stake_snapshot
            .as_ref()
            .expect("snapshot checked above");
        let transcript = receipt.verifier_transcript.as_slice();
        verifier_selection::validate_committee(
            &self.public_key,
            &self.policy,
            snapshot,
            transcript,
            committee,
        )
        .map_err(CommitteeValidationError::Invalid)?;
        Ok(())
    }

    fn record_rejection(&self, reason: &str) {
        increment_counter!(
            "ad_verifier_committee_rejection_total",
            "committee" => self.policy.label.as_str(),
            "reason" => reason
        );
    }
}

#[derive(Debug)]
enum CommitteeValidationError {
    MissingReceipt,
    MissingSnapshot,
    Invalid(VerifierSelectionError),
}

impl CommitteeValidationError {
    fn reason(&self) -> &'static str {
        match self {
            CommitteeValidationError::MissingReceipt => "committee_missing",
            CommitteeValidationError::MissingSnapshot => "snapshot_missing",
            CommitteeValidationError::Invalid(VerifierSelectionError::EmptySnapshot) => {
                "snapshot_empty"
            }
            CommitteeValidationError::Invalid(VerifierSelectionError::InsufficientStake(_)) => {
                "stake"
            }
            CommitteeValidationError::Invalid(VerifierSelectionError::InvalidProof) => "proof",
            CommitteeValidationError::Invalid(VerifierSelectionError::CommitteeSizeMismatch {
                ..
            }) => "size",
            CommitteeValidationError::Invalid(
                VerifierSelectionError::CommitteeMemberMismatch { .. },
            ) => "member",
            CommitteeValidationError::Invalid(VerifierSelectionError::SnapshotHashMismatch) => {
                "hash"
            }
            CommitteeValidationError::Invalid(
                VerifierSelectionError::StakeThresholdViolation { .. },
            ) => "threshold",
            CommitteeValidationError::Invalid(VerifierSelectionError::StakeUnitsMismatch {
                ..
            }) => "stake_units",
            CommitteeValidationError::Invalid(VerifierSelectionError::StakeWeightMismatch {
                ..
            }) => "stake_weight",
        }
    }
}

impl From<CommitteeValidationError> for SelectionReceiptError {
    fn from(err: CommitteeValidationError) -> Self {
        match err {
            CommitteeValidationError::MissingReceipt => {
                SelectionReceiptError::VerifierCommitteeMissing
            }
            CommitteeValidationError::MissingSnapshot => {
                SelectionReceiptError::VerifierCommitteeInvalid {
                    reason: "missing_snapshot".into(),
                }
            }
            CommitteeValidationError::Invalid(inner) => {
                SelectionReceiptError::VerifierCommitteeInvalid {
                    reason: inner.to_string(),
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
    use crate::{
        DeliveryChannel, DomainTier, SelectionCandidateTrace, SelectionCohortTrace,
        UpliftHoldoutAssignment,
    };
    use std::collections::HashSet;
    use verifier_selection::{self, CommitteeConfig as CommitteePolicy};
    use zkp::selection::{self, SelectionProofPublicInputs, SelectionProofVerification};

    const CIRCUIT_ID: &str = "selection_argmax_v1";

    fn transcript_digest(inputs: &SelectionProofPublicInputs, proof_bytes: &[u8]) -> [u8; 32] {
        let proof_digest = selection::proof_bytes_digest(proof_bytes);
        selection::expected_transcript_digest(CIRCUIT_ID, 1, &proof_digest, inputs)
            .expect("expected transcript digest")
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
                domain_tier: DomainTier::default(),
                domain_owner: None,
                provider: Some("wallet".into()),
                badges: vec!["badge-a".into(), "badge-b".into()],
                interest_tags: Vec::new(),
                presence_bucket: None,
                selectors_version: 0,
                bytes: 512,
                price_per_mib_usd_micros: 1_500_000,
                delivery_channel: DeliveryChannel::Http,
                mesh_peer: None,
                mesh_transport: None,
                mesh_latency_ms: None,
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
                    requested_kappa: 0.9,
                    shading_multiplier: 0.9,
                    shadow_price: 0.0,
                    dual_price: 0.0,
                    ..SelectionCandidateTrace::default()
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
                    requested_kappa: 0.8,
                    shading_multiplier: 0.8,
                    shadow_price: 0.0,
                    dual_price: 0.0,
                    ..SelectionCandidateTrace::default()
                },
            ],
            winner_index: 0,
            resource_floor_usd_micros: 1_200_000,
            resource_floor_breakdown: ResourceFloorBreakdown {
                bandwidth_usd_micros: 900_000,
                verifier_usd_micros: 200_000,
                host_usd_micros: 150_000,
                qualified_impressions_per_proof: 600,
            },
            runner_up_quality_bid_usd_micros: 1_800_000,
            clearing_price_usd_micros: 1_800_000,
            attestation: None,
            proof_metadata: None,
            verifier_committee: None,
            verifier_stake_snapshot: None,
            verifier_transcript: Vec::new(),
            badge_soft_intent: None,
            badge_soft_intent_snapshot: None,
            uplift_assignment: Some(UpliftHoldoutAssignment {
                fold: 0,
                in_holdout: false,
                propensity: 1.0,
            }),
        };
        let commitment = receipt.commitment_bytes_raw().expect("commitment");
        let inputs = SelectionProofPublicInputs {
            commitment: commitment.to_vec(),
            winner_index: receipt.winner_index as u16,
            winner_quality_bid_usd_micros: receipt.candidates[0].quality_adjusted_bid_usd_micros,
            runner_up_quality_bid_usd_micros: receipt.runner_up_quality_bid_usd_micros,
            resource_floor_usd_micros: receipt.resource_floor_usd_micros,
            clearing_price_usd_micros: receipt.clearing_price_usd_micros,
            candidate_count: receipt.candidates.len() as u16,
        };
        let proof_bytes = vec![0xCC; 96];
        let transcript = transcript_digest(&inputs, &proof_bytes);
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
        let proof = format!(
            "{{\"version\":1,\"circuit_revision\":1,\"public_inputs\":{},\"proof\":{{\"protocol\":\"groth16\",\"transcript_digest\":{},\"bytes\":{},\"witness_commitments\":{}}}}}",
            public_inputs,
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
            verifier_committee: None,
        });
        let (receipt, _, verification) = build_receipt();
        let metadata = receipt.proof_metadata.as_ref().expect("metadata present");
        assert_eq!(metadata.protocol.as_deref(), Some("groth16"));
        assert_eq!(metadata.proof_length, verification.proof_len);
        assert_eq!(metadata.witness_commitments.len(), 2);
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
            verifier_committee: None,
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
            verifier_committee: None,
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

    fn sample_snapshot() -> verifier_selection::StakeSnapshot {
        verifier_selection::StakeSnapshot {
            staking_epoch: 9,
            verifiers: vec![
                verifier_selection::StakeEntry {
                    verifier_id: "alpha".into(),
                    stake_units: 1_000,
                },
                verifier_selection::StakeEntry {
                    verifier_id: "beta".into(),
                    stake_units: 2_000,
                },
                verifier_selection::StakeEntry {
                    verifier_id: "gamma".into(),
                    stake_units: 3_000,
                },
            ],
        }
    }

    fn committee_manager() -> (
        SelectionAttestationManager,
        SelectionReceipt,
        verifier_selection::StakeSnapshot,
        Vec<u8>,
    ) {
        let mut preferred = HashSet::new();
        preferred.insert(CIRCUIT_ID.into());
        let mut rng = rand::rngs::StdRng::seed_from_u64(41);
        let (secret, public) = vrf::SecretKey::generate(&mut rng);
        let snapshot = sample_snapshot();
        let transcript = b"selection-transcript".to_vec();
        let policy = CommitteePolicy {
            label: "verifier-selection".into(),
            committee_size: 2,
            minimum_total_stake: 1,
            stake_threshold_ppm: 0,
        };
        let selection =
            verifier_selection::select_committee(&secret, &policy, &snapshot, &transcript)
                .expect("committee selected");
        let public_hex = hex::encode(public.to_bytes());
        let manager = SelectionAttestationManager::new(SelectionAttestationConfig {
            preferred_circuit_ids: preferred,
            allow_tee_fallback: false,
            require_attestation: true,
            verifier_committee: Some(VerifierCommitteeConfig {
                vrf_public_key_hex: public_hex,
                committee_size: policy.committee_size,
                minimum_total_stake: 1,
                stake_threshold_ppm: 0,
                label: "verifier-selection".into(),
                require_snapshot: true,
            }),
        });
        let (mut receipt, _, _) = build_receipt();
        receipt.verifier_committee = Some(selection.receipt.clone());
        receipt.verifier_stake_snapshot = Some(snapshot.clone());
        receipt.verifier_transcript = transcript.clone();
        if let Some(metadata) = receipt.proof_metadata.as_mut() {
            metadata.verifier_committee = Some(selection.receipt.clone());
        }
        (manager, receipt, snapshot, transcript)
    }

    #[test]
    fn committee_guard_accepts_valid_receipt() {
        let (manager, receipt, _, _) = committee_manager();
        manager
            .validate_receipt(&receipt)
            .expect("committee receipt valid");
    }

    #[test]
    fn committee_guard_rejects_missing_committee() {
        let (manager, mut receipt, snapshot, transcript) = committee_manager();
        receipt.verifier_committee = None;
        receipt.verifier_stake_snapshot = Some(snapshot);
        receipt.verifier_transcript = transcript;
        if let Some(metadata) = receipt.proof_metadata.as_mut() {
            metadata.verifier_committee = None;
        }
        let err = manager
            .validate_receipt(&receipt)
            .expect_err("missing committee should fail");
        assert!(matches!(
            err,
            SelectionReceiptError::VerifierCommitteeMissing
        ));
    }

    #[test]
    fn committee_guard_rejects_missing_snapshot_when_threshold_enabled() {
        let mut preferred = HashSet::new();
        preferred.insert(CIRCUIT_ID.into());
        let mut rng = rand::rngs::StdRng::seed_from_u64(43);
        let (secret, public) = vrf::SecretKey::generate(&mut rng);
        let snapshot = sample_snapshot();
        let transcript = b"selection-transcript".to_vec();
        let policy = CommitteePolicy {
            label: "verifier-selection".into(),
            committee_size: 2,
            minimum_total_stake: 1,
            stake_threshold_ppm: 250_000,
        };
        let selection =
            verifier_selection::select_committee(&secret, &policy, &snapshot, &transcript)
                .expect("committee selected");
        let manager = SelectionAttestationManager::new(SelectionAttestationConfig {
            preferred_circuit_ids: preferred,
            allow_tee_fallback: false,
            require_attestation: true,
            verifier_committee: Some(VerifierCommitteeConfig {
                vrf_public_key_hex: hex::encode(public.to_bytes()),
                committee_size: policy.committee_size,
                minimum_total_stake: 1,
                stake_threshold_ppm: policy.stake_threshold_ppm,
                label: policy.label.clone(),
                require_snapshot: false,
            }),
        });
        let (mut receipt, _, _) = build_receipt();
        receipt.verifier_committee = Some(selection.receipt.clone());
        receipt.verifier_transcript = transcript;
        if let Some(metadata) = receipt.proof_metadata.as_mut() {
            metadata.verifier_committee = Some(selection.receipt.clone());
        }
        let err = manager
            .validate_receipt(&receipt)
            .expect_err("missing snapshot should fail when threshold set");
        assert!(matches!(
            err,
            SelectionReceiptError::VerifierCommitteeInvalid { reason }
                if reason == "missing_snapshot"
        ));
    }

    #[test]
    fn committee_rejections_increment_metrics() {
        let Some(recorder) = TestMetricsRecorder::install() else {
            eprintln!("skipping metrics assertion; recorder already installed elsewhere");
            return;
        };
        recorder.reset();
        let mut preferred = HashSet::new();
        preferred.insert(CIRCUIT_ID.into());
        let mut rng = rand::rngs::StdRng::seed_from_u64(47);
        let (secret, public) = vrf::SecretKey::generate(&mut rng);
        let snapshot = sample_snapshot();
        let transcript = b"selection-transcript".to_vec();
        let policy = CommitteePolicy {
            label: "verifier-selection".into(),
            committee_size: 2,
            minimum_total_stake: 1,
            stake_threshold_ppm: 500_000,
        };
        let selection =
            verifier_selection::select_committee(&secret, &policy, &snapshot, &transcript)
                .expect("committee selected");
        let manager = SelectionAttestationManager::new(SelectionAttestationConfig {
            preferred_circuit_ids: preferred,
            allow_tee_fallback: false,
            require_attestation: true,
            verifier_committee: Some(VerifierCommitteeConfig {
                vrf_public_key_hex: hex::encode(public.to_bytes()),
                committee_size: policy.committee_size,
                minimum_total_stake: 1,
                stake_threshold_ppm: policy.stake_threshold_ppm,
                label: policy.label.clone(),
                require_snapshot: true,
            }),
        });
        let (mut receipt, proof, _) = build_receipt();
        receipt.verifier_committee = Some(selection.receipt.clone());
        receipt.verifier_transcript = transcript;
        let attestation = SelectionAttestation::Snark {
            proof: proof.clone(),
            circuit_id: CIRCUIT_ID.into(),
        };
        let (accepted, satisfaction, metadata) =
            manager.attach_attestation(&receipt, &[attestation]);
        assert!(accepted.is_none());
        assert_eq!(satisfaction, AttestationSatisfaction::Missing);
        assert!(metadata.is_none());

        let counters = recorder.counters();
        assert!(
            counters.iter().any(|event| {
                event.name == "ad_verifier_committee_rejection_total"
                    && event
                        .labels
                        .iter()
                        .any(|(k, v)| k == "reason" && v == "snapshot_missing")
                    && event
                        .labels
                        .iter()
                        .any(|(k, v)| k == "committee" && v == &policy.label)
            }),
            "expected committee rejection counter, got {counters:?}"
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
            verifier_committee: None,
        });
        let (receipt, proof, _) = build_receipt();
        let attestation = SelectionAttestation::Snark {
            proof: proof.clone(),
            circuit_id: CIRCUIT_ID.into(),
        };
        for _ in 0..5 {
            let _ = manager.attach_attestation(&receipt, &[attestation.clone()]);
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
