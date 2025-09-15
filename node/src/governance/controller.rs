use super::{GovStore, Params, Proposal, ProposalStatus, ReleaseBallot, ReleaseVote, Runtime};

/// Submit a proposal to the store and return its id.
pub fn submit_proposal(store: &GovStore, prop: Proposal) -> sled::Result<u64> {
    store.submit(prop)
}

/// Tally votes and queue proposals for activation.
pub fn tally(store: &GovStore, id: u64, epoch: u64) -> sled::Result<super::ProposalStatus> {
    store.tally_and_queue(id, epoch)
}

/// Submit a release vote proposal and return its id.
pub fn submit_release(store: &GovStore, mut prop: ReleaseVote) -> sled::Result<u64> {
    let requires_signature = crate::provenance::release_signature_required();
    prop.build_hash = prop.build_hash.to_lowercase();
    if let Some(sig) = prop.signature.as_ref() {
        if !crate::provenance::verify_release_signature(&prop.build_hash, sig) {
            return Err(sled::Error::Unsupported(
                "invalid provenance signature".into(),
            ));
        }
    } else if requires_signature {
        return Err(sled::Error::Unsupported(
            "missing provenance signature".into(),
        ));
    }
    store.submit_release(prop)
}

/// Record a vote for a release proposal.
pub fn vote_release(store: &GovStore, ballot: ReleaseBallot) -> sled::Result<()> {
    store.vote_release(ballot.proposal_id, ballot)
}

/// Tally a release proposal and persist approval state.
pub fn tally_release(store: &GovStore, id: u64, epoch: u64) -> sled::Result<ProposalStatus> {
    store.tally_release(id, epoch)
}

/// Return the set of approved releases.
pub fn approved_releases(store: &GovStore) -> sled::Result<Vec<super::ApprovedRelease>> {
    store.approved_release_hashes()
}

/// Record a local installation of an approved release hash.
pub fn record_release_install(store: &GovStore, hash: &str) -> sled::Result<()> {
    store.record_release_install(hash)
}

/// Return timestamps for local release installations keyed by hash.
pub fn release_installations(store: &GovStore) -> sled::Result<Vec<(String, u64)>> {
    store.release_installations()
}

/// Activate any proposals whose delay has elapsed.
pub fn activate_ready(
    store: &GovStore,
    epoch: u64,
    rt: &mut Runtime,
    params: &mut Params,
) -> sled::Result<()> {
    store.activate_ready(epoch, rt, params)
}

/// Roll back a proposal by id within the allowed window.
pub fn rollback(
    store: &GovStore,
    id: u64,
    epoch: u64,
    rt: &mut Runtime,
    params: &mut Params,
) -> sled::Result<()> {
    store.rollback_proposal(id, epoch, rt, params)
}
