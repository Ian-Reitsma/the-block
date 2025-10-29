#![forbid(unsafe_code)]

use core::cmp::Ordering;

use crypto_suite::hashing::blake3;
use crypto_suite::vrf::{self, OUTPUT_LENGTH};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryFrom;

#[cfg(feature = "telemetry")]
use foundation_metrics::{gauge, histogram};

const PPM_SCALE: f64 = 1_000_000.0;

/// Error conditions surfaced while computing or validating verifier committees.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum SelectionError {
    #[error("stake snapshot is empty")]
    EmptySnapshot,
    #[error("insufficient total stake: {0}")]
    InsufficientStake(u128),
    #[error("vrf proof invalid")]
    InvalidProof,
    #[error("committee size mismatch: expected {expected}, got {actual}")]
    CommitteeSizeMismatch { expected: usize, actual: usize },
    #[error("committee member mismatch for verifier {verifier_id}")]
    CommitteeMemberMismatch { verifier_id: String },
    #[error("snapshot hash mismatch")]
    SnapshotHashMismatch,
    #[error("stake threshold violation for verifier {verifier_id}")]
    StakeThresholdViolation { verifier_id: String },
}

/// Configuration describing how committees should be formed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CommitteeConfig {
    pub label: String,
    pub committee_size: u16,
    #[serde(default)]
    pub minimum_total_stake: u128,
    #[serde(default)]
    pub stake_threshold_ppm: u32,
}

impl CommitteeConfig {
    pub fn normalized(mut self) -> Self {
        if self.label.trim().is_empty() {
            self.label = "verifier-selection".into();
        }
        self.committee_size = self.committee_size.max(1);
        self.minimum_total_stake = self.minimum_total_stake.max(1);
        self
    }
}

/// Snapshot of verifier staking weights used for committee derivation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StakeSnapshot {
    pub staking_epoch: u64,
    pub verifiers: Vec<StakeEntry>,
}

impl StakeSnapshot {
    pub fn total_stake(&self) -> u128 {
        self.verifiers
            .iter()
            .map(|entry| entry.stake_units as u128)
            .sum()
    }

    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.staking_epoch.to_le_bytes());
        hasher.update(&(self.verifiers.len() as u32).to_le_bytes());
        for entry in &self.verifiers {
            let id_bytes = entry.verifier_id.as_bytes();
            hasher.update(&(id_bytes.len() as u32).to_le_bytes());
            hasher.update(id_bytes);
            hasher.update(&entry.stake_units.to_le_bytes());
        }
        hasher.finalize().into()
    }
}

/// Staking position associated with a verifier.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StakeEntry {
    pub verifier_id: String,
    pub stake_units: u64,
}

/// Committee member information embedded in selection receipts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CommitteeMemberReceipt {
    pub verifier_id: String,
    pub stake_units: u64,
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub selection_hash: Vec<u8>,
    pub weight_ppm: u32,
}

/// Receipt describing the VRF output and committee that was derived from a stake snapshot.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct SelectionReceipt {
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub vrf_output: Vec<u8>,
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub vrf_proof: Vec<u8>,
    pub committee: Vec<CommitteeMemberReceipt>,
    pub committee_size: u16,
    #[serde(with = "foundation_serialization::serde_bytes", default)]
    pub stake_snapshot_hash: Vec<u8>,
    pub staking_epoch: u64,
}

impl SelectionReceipt {
    pub fn ensure_consistency(&self) -> Result<(), SelectionError> {
        if self.committee_size as usize != self.committee.len() {
            return Err(SelectionError::CommitteeSizeMismatch {
                expected: self.committee_size as usize,
                actual: self.committee.len(),
            });
        }
        Ok(())
    }
}

/// Result of running the VRF-backed selection algorithm.
#[derive(Clone, Debug)]
pub struct CommitteeSelection {
    pub receipt: SelectionReceipt,
    pub output: vrf::Output,
    pub proof: vrf::Proof,
}

impl CommitteeSelection {
    pub fn into_receipt(self) -> SelectionReceipt {
        self.receipt
    }
}

/// Deterministically derive a verifier committee from the stake snapshot using the provided VRF key.
pub fn select_committee(
    secret_key: &vrf::SecretKey,
    config: &CommitteeConfig,
    snapshot: &StakeSnapshot,
    external_transcript: &[u8],
) -> Result<CommitteeSelection, SelectionError> {
    let normalized = config.clone().normalized();
    if snapshot.verifiers.is_empty() {
        return Err(SelectionError::EmptySnapshot);
    }
    let total_stake = snapshot.total_stake();
    if total_stake < normalized.minimum_total_stake {
        return Err(SelectionError::InsufficientStake(total_stake));
    }
    let transcript_seed = build_transcript_seed(&normalized.label, snapshot, external_transcript);
    let (output, proof) =
        secret_key.evaluate(normalized.label.as_bytes(), transcript_seed.as_bytes());
    let priorities = compute_priorities(output.as_bytes(), snapshot);
    let mut ranked: Vec<_> = priorities.into_iter().collect();
    ranked.sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
    let chosen = ranked
        .into_iter()
        .take(normalized.committee_size as usize)
        .collect::<Vec<_>>();
    #[cfg(feature = "telemetry")]
    {
        gauge!(
            "verifier_selection_committee_size",
            normalized.committee_size as f64,
            "label" => normalized.label.as_str()
        );
        histogram!(
            "verifier_selection_total_stake",
            total_stake as f64,
            "label" => normalized.label.as_str()
        );
    }
    let receipt = build_receipt(snapshot, &normalized, &chosen, output, &proof);
    Ok(CommitteeSelection {
        receipt,
        output,
        proof,
    })
}

/// Validate the provided receipt against the VRF public key, stake snapshot, and configuration.
pub fn validate_committee(
    public_key: &vrf::PublicKey,
    config: &CommitteeConfig,
    snapshot: &StakeSnapshot,
    external_transcript: &[u8],
    receipt: &SelectionReceipt,
) -> Result<(), SelectionError> {
    receipt.ensure_consistency()?;
    let normalized = config.clone().normalized();
    let expected_hash = snapshot.hash();
    if receipt.stake_snapshot_hash.as_slice() != expected_hash.as_ref() {
        return Err(SelectionError::SnapshotHashMismatch);
    }
    let transcript_seed = build_transcript_seed(&normalized.label, snapshot, external_transcript);
    let proof = vrf::Proof::try_from(receipt.vrf_proof.as_slice())
        .map_err(|_| SelectionError::InvalidProof)?;
    let output = public_key
        .verify(
            normalized.label.as_bytes(),
            transcript_seed.as_bytes(),
            &proof,
        )
        .map_err(|_| SelectionError::InvalidProof)?;
    if receipt.vrf_output.as_slice() != output.as_bytes() {
        return Err(SelectionError::InvalidProof);
    }
    let priorities = compute_priorities(output.as_bytes(), snapshot);
    let mut ranked: Vec<_> = priorities.into_iter().collect();
    ranked.sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
    for (idx, (verifier_id, _priority)) in ranked
        .into_iter()
        .take(normalized.committee_size as usize)
        .enumerate()
    {
        let Some(member) = receipt.committee.get(idx) else {
            return Err(SelectionError::CommitteeSizeMismatch {
                expected: normalized.committee_size as usize,
                actual: receipt.committee.len(),
            });
        };
        if member.verifier_id != verifier_id {
            return Err(SelectionError::CommitteeMemberMismatch { verifier_id });
        }
        if normalized.stake_threshold_ppm > 0 {
            let weight = member.weight_ppm as u128;
            if weight < normalized.stake_threshold_ppm as u128 {
                return Err(SelectionError::StakeThresholdViolation {
                    verifier_id: member.verifier_id.clone(),
                });
            }
        }
    }
    #[cfg(feature = "telemetry")]
    {
        gauge!("verifier_selection_validation", 1.0, "label" => normalized.label.as_str());
    }
    Ok(())
}

fn build_transcript_seed(label: &str, snapshot: &StakeSnapshot, external: &[u8]) -> blake3::Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(label.as_bytes());
    hasher.update(&snapshot.hash());
    hasher.update(&snapshot.staking_epoch.to_le_bytes());
    hasher.update(external);
    hasher.finalize()
}

fn compute_priorities(
    output: &[u8; OUTPUT_LENGTH],
    snapshot: &StakeSnapshot,
) -> HashMap<String, f64> {
    let mut priorities = HashMap::with_capacity(snapshot.verifiers.len());
    for entry in &snapshot.verifiers {
        let mut hasher = blake3::Hasher::new();
        hasher.update(output);
        hasher.update(entry.verifier_id.as_bytes());
        hasher.update(&entry.stake_units.to_le_bytes());
        let digest = hasher.finalize();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&digest.as_bytes()[..8]);
        let raw = u64::from_le_bytes(bytes) as f64 / u64::MAX as f64;
        let priority = raw / (entry.stake_units.max(1) as f64);
        priorities.insert(entry.verifier_id.clone(), priority);
    }
    priorities
}

fn build_receipt(
    snapshot: &StakeSnapshot,
    config: &CommitteeConfig,
    ranked: &[(String, f64)],
    output: vrf::Output,
    proof: &vrf::Proof,
) -> SelectionReceipt {
    let mut committee = Vec::with_capacity(ranked.len());
    let total_stake = snapshot.total_stake().max(1);
    for (verifier_id, priority) in ranked {
        let stake_units = snapshot
            .verifiers
            .iter()
            .find(|entry| &entry.verifier_id == verifier_id)
            .map(|entry| entry.stake_units)
            .unwrap_or_default();
        let weight = ((stake_units as f64 / total_stake as f64) * PPM_SCALE)
            .round()
            .clamp(0.0, PPM_SCALE) as u32;
        let mut hash = vec![0u8; 16];
        hash[..8].copy_from_slice(&priority.to_bits().to_le_bytes());
        hash[8..].copy_from_slice(&stake_units.to_le_bytes());
        committee.push(CommitteeMemberReceipt {
            verifier_id: verifier_id.clone(),
            stake_units,
            selection_hash: hash,
            weight_ppm: weight,
        });
    }
    SelectionReceipt {
        vrf_output: output.into_bytes().to_vec(),
        vrf_proof: proof.to_bytes().to_vec(),
        committee,
        committee_size: config.committee_size,
        stake_snapshot_hash: snapshot.hash().to_vec(),
        staking_epoch: snapshot.staking_epoch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> StakeSnapshot {
        StakeSnapshot {
            staking_epoch: 42,
            verifiers: vec![
                StakeEntry {
                    verifier_id: "alpha".into(),
                    stake_units: 1_000,
                },
                StakeEntry {
                    verifier_id: "beta".into(),
                    stake_units: 2_000,
                },
                StakeEntry {
                    verifier_id: "gamma".into(),
                    stake_units: 4_000,
                },
            ],
        }
    }

    #[test]
    fn committee_round_trip() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let (secret, public) = vrf::SecretKey::generate(&mut rng);
        let config = CommitteeConfig {
            label: "selection".into(),
            committee_size: 2,
            minimum_total_stake: 1,
            stake_threshold_ppm: 0,
        };
        let snapshot = sample_snapshot();
        let selection = select_committee(&secret, &config, &snapshot, b"transcript").unwrap();
        validate_committee(
            &public,
            &config,
            &snapshot,
            b"transcript",
            &selection.receipt,
        )
        .unwrap();
    }

    #[test]
    fn rejects_modified_committee() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(9);
        let (secret, public) = vrf::SecretKey::generate(&mut rng);
        let config = CommitteeConfig {
            label: "selection".into(),
            committee_size: 2,
            minimum_total_stake: 1,
            stake_threshold_ppm: 0,
        };
        let snapshot = sample_snapshot();
        let mut receipt = select_committee(&secret, &config, &snapshot, b"transcript")
            .unwrap()
            .into_receipt();
        receipt.committee.reverse();
        let err = validate_committee(&public, &config, &snapshot, b"transcript", &receipt)
            .expect_err("reverse should fail");
        assert!(matches!(
            err,
            SelectionError::CommitteeMemberMismatch { .. }
        ));
    }
}
