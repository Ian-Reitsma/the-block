use super::{ParamKey, Proposal, ProposalStatus, Vote, VoteChoice, Params, registry};
use sled::Config;
use std::path::Path;
use serde::{Serialize, Deserialize};

pub const ACTIVATION_DELAY: u64 = 2;
pub const ROLLBACK_WINDOW_EPOCHS: u64 = 1;
pub const QUORUM: u64 = 1;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LastActivation {
    pub proposal_id: u64,
    pub key: ParamKey,
    pub old_value: i64,
    pub new_value: i64,
    pub activated_epoch: u64,
}

pub struct GovStore {
    db: sled::Db,
}

impl GovStore {
    pub fn open(path: impl AsRef<Path>) -> Self {
        let db = Config::new().path(path).open().expect("open db");
        Self { db }
    }

    pub(crate) fn proposals(&self) -> sled::Tree { self.db.open_tree("proposals").unwrap() }
    fn votes(&self, id: u64) -> sled::Tree { self.db.open_tree(format!("votes/{id}")).unwrap() }
    fn next_id(&self) -> sled::Tree { self.db.open_tree("next_id").unwrap() }
    fn active_params(&self) -> sled::Tree { self.db.open_tree("active_params").unwrap() }
    fn activation_queue(&self) -> sled::Tree { self.db.open_tree("activation_queue").unwrap() }
    fn last_activation(&self) -> sled::Tree { self.db.open_tree("last_activation").unwrap() }

    pub fn submit(&self, mut p: Proposal) -> sled::Result<u64> {
        if p.new_value < p.min || p.new_value > p.max { return Err(sled::Error::Unsupported("out of bounds".into())); }
        let next = self.next_id().get("id")?.map(|v| bincode::deserialize::<u64>(&v).unwrap()).unwrap_or(0);
        self.next_id().insert("id", bincode::serialize(&(next+1)).unwrap())?;
        p.id = next;
        self.proposals().insert(bincode::serialize(&p.id).unwrap(), bincode::serialize(&p).unwrap())?;
        Ok(next)
    }

    pub fn vote(&self, proposal_id: u64, mut v: Vote, current_epoch: u64) -> sled::Result<()> {
        let prop_raw = self.proposals().get(bincode::serialize(&proposal_id).unwrap())?.expect("missing proposal");
        let prop: Proposal = bincode::deserialize(&prop_raw).unwrap();
        if current_epoch >= prop.vote_deadline_epoch { return Err(sled::Error::Unsupported("deadline".into())); }
        v.received_at = current_epoch;
        self.votes(proposal_id).insert(v.voter.as_bytes(), bincode::serialize(&v).unwrap())?;
        Ok(())
    }

    pub fn tally_and_queue(&self, proposal_id: u64, current_epoch: u64) -> sled::Result<ProposalStatus> {
        let key = bincode::serialize(&proposal_id).unwrap();
        let mut prop: Proposal = bincode::deserialize(&self.proposals().get(&key)?.expect("missing proposal")).unwrap();
        if prop.status != ProposalStatus::Open { return Ok(prop.status); }
        if current_epoch < prop.vote_deadline_epoch { return Ok(ProposalStatus::Open); }
        let votes = self.votes(proposal_id);
        let mut yes = 0u64; let mut no = 0u64;
        for v in votes.iter() {
            let (_, raw) = v?; let vote: Vote = bincode::deserialize(&raw).unwrap();
            match vote.choice { VoteChoice::Yes => yes+=vote.weight, VoteChoice::No => no+=vote.weight, _ => {} }
        }
        if yes >= QUORUM && yes > no {
            prop.status = ProposalStatus::Passed;
            let act_epoch = current_epoch + ACTIVATION_DELAY;
            prop.activation_epoch = Some(act_epoch);
            let key_epoch = bincode::serialize(&act_epoch).unwrap();
            let mut list: Vec<u64> = self.activation_queue().get(&key_epoch)?.map(|v| bincode::deserialize(&v).unwrap()).unwrap_or_else(|| vec![]);
            list.push(proposal_id);
            self.activation_queue().insert(key_epoch, bincode::serialize(&list).unwrap())?;
        } else {
            prop.status = ProposalStatus::Rejected;
        }
        self.proposals().insert(&key, bincode::serialize(&prop).unwrap())?;
        Ok(prop.status)
    }

    pub fn activate_ready(&self, current_epoch: u64, params: &mut Params) -> sled::Result<()> {
        let queue = self.activation_queue();
        let mut to_remove = vec![];
        for item in queue.iter() {
            let (k, v) = item?;
            let epoch: u64 = bincode::deserialize(&k).unwrap();
            if epoch <= current_epoch {
                let ids: Vec<u64> = bincode::deserialize(&v).unwrap_or_default();
                for prop_id in ids {
                    let key = bincode::serialize(&prop_id).unwrap();
                    if let Some(raw) = self.proposals().get(&key)? {
                        let mut prop: Proposal = bincode::deserialize(&raw).unwrap();
                        if prop.status == ProposalStatus::Passed {
                            let old = match prop.key {
                                ParamKey::SnapshotIntervalSecs => params.snapshot_interval_secs,
                                ParamKey::ConsumerFeeComfortP90Microunits => params.consumer_fee_comfort_p90_microunits,
                                ParamKey::IndustrialAdmissionMinCapacity => params.industrial_admission_min_capacity,
                            };
                            if let Some(spec) = registry().iter().find(|s| s.key == prop.key) {
                                (spec.apply)(prop.new_value, params).map_err(|_| sled::Error::Unsupported("apply".into()))?;
                            }
                            let last = LastActivation {
                                proposal_id: prop.id,
                                key: prop.key,
                                old_value: old,
                                new_value: prop.new_value,
                                activated_epoch: current_epoch,
                            };
                            self.last_activation()
                                .insert("last", bincode::serialize(&last).unwrap())?;
                            prop.status = ProposalStatus::Activated;
                            self.proposals().insert(&key, bincode::serialize(&prop).unwrap())?;
                            self.active_params().insert(
                                bincode::serialize(&prop.key).unwrap(),
                                bincode::serialize(&prop.new_value).unwrap(),
                            )?;
                        }
                    }
                }
                to_remove.push(epoch);
            }
        }
        for e in to_remove {
            queue.remove(bincode::serialize(&e).unwrap())?;
        }
        Ok(())
    }

    pub fn rollback_last(&self, current_epoch: u64, params: &mut Params) -> sled::Result<()> {
        if let Some(raw) = self.last_activation().get("last")? {
            let last: LastActivation = bincode::deserialize(&raw).unwrap();
            if current_epoch > last.activated_epoch + ROLLBACK_WINDOW_EPOCHS { return Err(sled::Error::Unsupported("expired".into())); }
            if let Some(spec) = registry().iter().find(|s| s.key == last.key) {
                (spec.apply)(last.old_value, params).map_err(|_| sled::Error::Unsupported("apply".into()))?;
            }
            self.active_params().insert(bincode::serialize(&last.key).unwrap(), bincode::serialize(&last.old_value).unwrap())?;
            if let Some(prop_raw) = self.proposals().get(bincode::serialize(&last.proposal_id).unwrap())? {
                let mut prop: Proposal = bincode::deserialize(&prop_raw).unwrap();
                prop.status = ProposalStatus::RolledBack;
                self.proposals().insert(bincode::serialize(&prop.id).unwrap(), bincode::serialize(&prop).unwrap())?;
            }
            self.last_activation().remove("last")?;
            return Ok(());
        }
        Err(sled::Error::ReportableBug("no activation".into()))
    }
}

