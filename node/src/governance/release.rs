use super::{Address, GovStore, ProposalStatus, VoteChoice, QUORUM};
use serde::{Deserialize, Serialize};

/// Governance proposal representing a release hash endorsement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseVote {
    pub id: u64,
    /// Hex-encoded BLAKE3 hash of the release artifact.
    pub build_hash: String,
    /// Optional detached signature attesting to the build provenance.
    pub signature: Option<String>,
    pub proposer: Address,
    pub created_epoch: u64,
    pub vote_deadline_epoch: u64,
    pub activation_epoch: Option<u64>,
    pub status: ProposalStatus,
}

impl ReleaseVote {
    pub fn new(
        build_hash: String,
        signature: Option<String>,
        proposer: Address,
        created_epoch: u64,
        vote_deadline_epoch: u64,
    ) -> Self {
        let normalized = build_hash.to_lowercase();
        Self {
            id: 0,
            build_hash: normalized,
            signature,
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
