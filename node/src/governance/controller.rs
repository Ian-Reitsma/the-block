use super::{GovStore, Params, Proposal, Runtime};

/// Submit a proposal to the store and return its id.
pub fn submit_proposal(store: &GovStore, prop: Proposal) -> sled::Result<u64> {
    store.submit(prop)
}

/// Tally votes and queue proposals for activation.
pub fn tally(store: &GovStore, id: u64, epoch: u64) -> sled::Result<super::ProposalStatus> {
    store.tally_and_queue(id, epoch)
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
