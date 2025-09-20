use super::{
    GovStore, Params, Proposal, ProposalStatus, ReleaseBallot, ReleaseVerifier, ReleaseVote,
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
pub fn submit_release<V: ReleaseVerifier + ?Sized>(
    store: &GovStore,
    mut prop: ReleaseVote,
    verifier: Option<&V>,
) -> sled::Result<u64> {
    prop.build_hash = prop.build_hash.to_lowercase();
    prop.signer_set = prop
        .signer_set
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect();
    prop.signer_set.sort();
    prop.signer_set.dedup();

    if let Some(verifier) = verifier {
        let mut configured: Vec<String> = verifier
            .configured_signers()
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect();
        configured.sort();
        configured.dedup();
        if prop.signer_set.is_empty() {
            prop.signer_set = configured.clone();
        }
        if prop.signature_threshold == 0 && !prop.signer_set.is_empty() {
            prop.signature_threshold = prop.signer_set.len() as u32;
        } else if prop.signature_threshold == 0 && !configured.is_empty() {
            prop.signature_threshold = configured.len() as u32;
        }

        if !prop.signer_set.is_empty() && prop.signature_threshold as usize > prop.signer_set.len()
        {
            return Err(sled::Error::Unsupported(
                "threshold exceeds signer set".into(),
            ));
        }

        let allowed: HashSet<String> = if prop.signer_set.is_empty() {
            configured.into_iter().collect()
        } else {
            prop.signer_set.iter().cloned().collect()
        };

        if prop.signature_threshold > 0 && allowed.is_empty() {
            return Err(sled::Error::Unsupported(
                "no release signers configured".into(),
            ));
        }

        let mut seen: HashSet<String> = HashSet::new();
        let mut valid = 0usize;
        for att in &prop.signatures {
            let signer = att.signer.to_lowercase();
            if !allowed.contains(&signer) {
                return Err(sled::Error::Unsupported(
                    "invalid provenance attestation".into(),
                ));
            }
            if !verifier.verify(&prop.build_hash, &signer, &att.signature) {
                return Err(sled::Error::Unsupported(
                    "invalid provenance attestation".into(),
                ));
            }
            if seen.insert(signer) {
                valid += 1;
            }
        }
        if prop.signature_threshold as usize > 0 && valid < prop.signature_threshold as usize {
            return Err(sled::Error::Unsupported(
                "insufficient release signers".into(),
            ));
        }
    } else {
        if prop.signature_threshold == 0 && !prop.signer_set.is_empty() {
            prop.signature_threshold = prop.signer_set.len() as u32;
        }
        if !prop.signer_set.is_empty() && prop.signature_threshold as usize > prop.signer_set.len()
        {
            return Err(sled::Error::Unsupported(
                "threshold exceeds signer set".into(),
            ));
        }
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
