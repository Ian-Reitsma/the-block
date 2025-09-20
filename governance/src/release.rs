use super::{Address, GovStore, ProposalStatus, VoteChoice, QUORUM};
use serde::{Deserialize, Serialize};

/// Provenance attestation over a release artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseAttestation {
    /// Hex-encoded Ed25519 verifying key that produced the signature.
    pub signer: String,
    /// Hex-encoded signature over the release hash payload.
    pub signature: String,
}

/// Governance proposal representing a release hash endorsement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseVote {
    pub id: u64,
    /// Hex-encoded BLAKE3 hash of the release artifact.
    pub build_hash: String,
    /// Set of provenance attestations collected for the build.
    pub signatures: Vec<ReleaseAttestation>,
    /// Number of unique signer approvals required for submission.
    pub signature_threshold: u32,
    /// Snapshot of the configured signer set at submission time.
    pub signer_set: Vec<String>,
    pub proposer: Address,
    pub created_epoch: u64,
    pub vote_deadline_epoch: u64,
    pub activation_epoch: Option<u64>,
    pub status: ProposalStatus,
}

impl ReleaseVote {
    pub fn new(
        build_hash: String,
        signatures: Vec<ReleaseAttestation>,
        signature_threshold: u32,
        proposer: Address,
        created_epoch: u64,
        vote_deadline_epoch: u64,
    ) -> Self {
        let normalized = build_hash.to_lowercase();
        let mut seen = std::collections::HashSet::new();
        let mut attestations = Vec::new();
        for mut att in signatures {
            let signer_norm = att.signer.to_lowercase();
            if seen.insert(signer_norm.clone()) {
                att.signer = signer_norm;
                attestations.push(att);
            }
        }
        Self {
            id: 0,
            build_hash: normalized,
            signatures: attestations,
            signature_threshold,
            signer_set: Vec::new(),
            proposer,
            created_epoch,
            vote_deadline_epoch,
            activation_epoch: None,
            status: ProposalStatus::Open,
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self.status, ProposalStatus::Open | ProposalStatus::Passed)
    }

    pub fn mark_passed(&mut self, epoch: u64) {
        self.status = ProposalStatus::Passed;
        self.activation_epoch = Some(epoch);
    }

    pub fn mark_activated(&mut self, epoch: u64) {
        self.status = ProposalStatus::Activated;
        self.activation_epoch = Some(epoch);
    }

    pub fn mark_rejected(&mut self) {
        self.status = ProposalStatus::Rejected;
    }

    pub fn quorum_met(yes_weight: u64) -> bool {
        yes_weight >= QUORUM
    }
}

/// Ballot cast for a release proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseBallot {
    pub proposal_id: u64,
    pub voter: Address,
    pub choice: VoteChoice,
    pub weight: u64,
    pub received_at: u64,
}

impl ReleaseBallot {
    pub fn yes(id: u64, voter: Address, weight: u64, epoch: u64) -> Self {
        Self {
            proposal_id: id,
            voter,
            choice: VoteChoice::Yes,
            weight,
            received_at: epoch,
        }
    }
}

/// Record describing an activated release hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovedRelease {
    pub build_hash: String,
    pub activated_epoch: u64,
    pub proposer: Address,
    pub signatures: Vec<ReleaseAttestation>,
    pub signature_threshold: u32,
    pub signer_set: Vec<String>,
    /// Local install timestamps recorded for this release.
    pub install_times: Vec<u64>,
}

/// Runtime hook used to validate release attestations when submitting proposals.
pub trait ReleaseVerifier {
    /// Return the canonical signer set configured for release approvals.
    fn configured_signers(&self) -> Vec<String>;

    /// Validate an attestation over `build_hash`.
    fn verify(&self, build_hash: &str, signer_hex: &str, signature_hex: &str) -> bool;
}

/// Resolve the on-disk path for the governance database.
fn store_path() -> String {
    std::env::var("TB_GOV_DB_PATH").unwrap_or_else(|_| "governance_db".into())
}

/// Ensure the provided release hash has been approved on-chain before startup.
pub fn ensure_release_authorized(build_hash: &str) -> Result<(), String> {
    let store = GovStore::open(store_path());
    if !store
        .is_release_hash_approved(build_hash)
        .map_err(|e| e.to_string())?
    {
        return Err(format!("binary hash {build_hash} is not approved"));
    }
    store
        .record_release_install(build_hash)
        .map_err(|e| e.to_string())
}

/// Return all approved release records for observability.
pub fn approved_releases() -> Vec<ApprovedRelease> {
    let store = GovStore::open(store_path());
    store.approved_release_hashes().unwrap_or_default()
}
