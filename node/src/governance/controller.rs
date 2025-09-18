use super::{
    GovStore, Params, Proposal, ProposalStatus, ReleaseAttestation, ReleaseBallot, ReleaseVote,
    Runtime,
};
use std::collections::HashSet;

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
    prop.build_hash = prop.build_hash.to_lowercase();
    let configured_signers = crate::provenance::release_signer_keys();
    if prop.signature_threshold == 0 && !configured_signers.is_empty() {
        prop.signature_threshold = configured_signers.len() as u32;
    }
    if !configured_signers.is_empty() {
        let configured_lookup: HashSet<[u8; 32]> =
            configured_signers.iter().map(|vk| vk.to_bytes()).collect();
        let mut seen: HashSet<[u8; 32]> = HashSet::new();
        let mut valid = 0usize;
        for ReleaseAttestation { signer, signature } in &prop.signatures {
            let Some(vk) = crate::provenance::parse_signer_hex(signer) else {
                return Err(sled::Error::Unsupported(
                    "invalid provenance attestation".into(),
                ));
            };
            let signer_bytes = vk.to_bytes();
            if !configured_lookup.contains(&signer_bytes) {
                return Err(sled::Error::Unsupported(
                    "invalid provenance attestation".into(),
                ));
            }
            if crate::provenance::verify_release_attestation(&prop.build_hash, &vk, signature) {
                if seen.insert(signer_bytes) {
                    valid += 1;
                }
            } else {
                return Err(sled::Error::Unsupported(
                    "invalid provenance attestation".into(),
                ));
            }
        }
        if valid < prop.signature_threshold as usize {
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::RELEASE_QUORUM_FAIL_TOTAL.inc();
            }
            return Err(sled::Error::Unsupported(
                "insufficient release signers".into(),
            ));
        }
    } else if prop.signature_threshold > 0 {
        // No configured signers, but caller requested a threshold; reject.
        return Err(sled::Error::Unsupported(
            "no release signers configured".into(),
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
pub fn release_installations(store: &GovStore) -> sled::Result<Vec<(String, Vec<u64>)>> {
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
