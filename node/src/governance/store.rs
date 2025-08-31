use super::{registry, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote, VoteChoice};
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    governance_webhook, GOV_ACTIVATION_DELAY_SECONDS, GOV_ROLLBACK_TOTAL, GOV_VOTES_TOTAL,
    PARAM_CHANGE_ACTIVE, PARAM_CHANGE_PENDING,
};
#[cfg(feature = "telemetry")]
use log::info;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sled::Config;
use std::path::Path;

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
    base_path: std::path::PathBuf,
}

fn ser<T: Serialize>(value: &T) -> sled::Result<Vec<u8>> {
    bincode::serialize(value).map_err(|e| sled::Error::Unsupported(format!("ser: {e}").into()))
}

fn de<T: DeserializeOwned>(bytes: &[u8]) -> sled::Result<T> {
    bincode::deserialize(bytes).map_err(|e| sled::Error::Unsupported(format!("de: {e}").into()))
}

#[cfg(feature = "telemetry")]
fn key_name(k: ParamKey) -> &'static str {
    match k {
        ParamKey::SnapshotIntervalSecs => "snapshot_interval_secs",
        ParamKey::ConsumerFeeComfortP90Microunits => "consumer_fee_comfort_p90_microunits",
        ParamKey::IndustrialAdmissionMinCapacity => "industrial_admission_min_capacity",
        ParamKey::FairshareGlobalMax => "fairshare_global_max_ppm",
        ParamKey::BurstRefillRatePerS => "burst_refill_rate_per_s_ppm",
        ParamKey::CreditsDecayLambdaPerHourPpm => "credits_decay_lambda_per_hour_ppm",
        ParamKey::DailyPayoutCap => "daily_payout_cap",
    }
}

impl GovStore {
    pub fn open(path: impl AsRef<Path>) -> Self {
        let db_path = path.as_ref();
        let db = Config::new()
            .path(db_path)
            .open()
            .unwrap_or_else(|e| panic!("open db: {e}"));
        let base = db_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| Path::new(".").to_path_buf());
        Self {
            db,
            base_path: base,
        }
    }

    pub(crate) fn proposals(&self) -> sled::Tree {
        self.db
            .open_tree("proposals")
            .unwrap_or_else(|e| panic!("open proposals tree: {e}"))
    }
    fn votes(&self, id: u64) -> sled::Tree {
        self.db
            .open_tree(format!("votes/{id}"))
            .unwrap_or_else(|e| panic!("open votes tree: {e}"))
    }
    fn next_id(&self) -> sled::Tree {
        self.db
            .open_tree("next_id")
            .unwrap_or_else(|e| panic!("open next_id tree: {e}"))
    }
    fn active_params(&self) -> sled::Tree {
        self.db
            .open_tree("active_params")
            .unwrap_or_else(|e| panic!("open active_params tree: {e}"))
    }
    fn activation_queue(&self) -> sled::Tree {
        self.db
            .open_tree("activation_queue")
            .unwrap_or_else(|e| panic!("open activation_queue tree: {e}"))
    }
    fn last_activation(&self) -> sled::Tree {
        self.db
            .open_tree("last_activation")
            .unwrap_or_else(|e| panic!("open last_activation tree: {e}"))
    }

    pub fn submit(&self, mut p: Proposal) -> sled::Result<u64> {
        if p.new_value < p.min || p.new_value > p.max {
            return Err(sled::Error::Unsupported("out of bounds".into()));
        }
        let next = self
            .next_id()
            .get("id")?
            .map(|v| de::<u64>(&v))
            .transpose()?
            .unwrap_or(0);
        self.next_id().insert("id", ser(&(next + 1))?)?;
        p.id = next;
        self.proposals().insert(ser(&p.id)?, ser(&p)?)?;
        Ok(next)
    }

    pub fn vote(&self, proposal_id: u64, mut v: Vote, current_epoch: u64) -> sled::Result<()> {
        let prop_raw = self
            .proposals()
            .get(ser(&proposal_id)?)?
            .ok_or_else(|| sled::Error::Unsupported("missing proposal".into()))?;
        let prop: Proposal = de(&prop_raw)?;
        if current_epoch >= prop.vote_deadline_epoch {
            return Err(sled::Error::Unsupported("deadline".into()));
        }
        v.received_at = current_epoch;
        self.votes(proposal_id)
            .insert(v.voter.as_bytes(), ser(&v)?)?;
        #[cfg(feature = "telemetry")]
        {
            let choice = match v.choice {
                VoteChoice::Yes => "yes",
                VoteChoice::No => "no",
                VoteChoice::Abstain => "abstain",
            };
            GOV_VOTES_TOTAL.with_label_values(&[choice]).inc();
            governance_webhook("vote", proposal_id);
        }
        Ok(())
    }

    pub fn tally_and_queue(
        &self,
        proposal_id: u64,
        current_epoch: u64,
    ) -> sled::Result<ProposalStatus> {
        let key = ser(&proposal_id)?;
        let mut prop: Proposal = de(&self
            .proposals()
            .get(&key)?
            .ok_or_else(|| sled::Error::Unsupported("missing proposal".into()))?)?;
        if prop.status != ProposalStatus::Open {
            return Ok(prop.status);
        }
        if current_epoch < prop.vote_deadline_epoch {
            return Ok(ProposalStatus::Open);
        }
        let votes = self.votes(proposal_id);
        let mut yes = 0u64;
        let mut no = 0u64;
        for v in votes.iter() {
            let (_, raw) = v?;
            let vote: Vote = de(&raw)?;
            match vote.choice {
                VoteChoice::Yes => yes += vote.weight,
                VoteChoice::No => no += vote.weight,
                _ => {}
            }
        }
        if yes >= QUORUM && yes > no {
            prop.status = ProposalStatus::Passed;
            let act_epoch = current_epoch + ACTIVATION_DELAY;
            prop.activation_epoch = Some(act_epoch);
            let key_epoch = ser(&act_epoch)?;
            let mut list: Vec<u64> = self
                .activation_queue()
                .get(&key_epoch)?
                .map(|v| de(&v))
                .transpose()?
                .unwrap_or_else(|| vec![]);
            list.push(proposal_id);
            self.activation_queue().insert(key_epoch, ser(&list)?)?;
            #[cfg(feature = "telemetry")]
            {
                PARAM_CHANGE_PENDING
                    .with_label_values(&[key_name(prop.key)])
                    .set(1);
            }
        } else {
            prop.status = ProposalStatus::Rejected;
            #[cfg(feature = "telemetry")]
            {
                PARAM_CHANGE_PENDING
                    .with_label_values(&[key_name(prop.key)])
                    .set(0);
            }
        }
        self.proposals().insert(&key, ser(&prop)?)?;
        Ok(prop.status)
    }

    pub fn activate_ready(
        &self,
        current_epoch: u64,
        rt: &mut Runtime,
        params: &mut Params,
    ) -> sled::Result<()> {
        // snapshot current params before applying any changes
        let hist_dir = self.base_path.join("governance/history");
        let _ = std::fs::create_dir_all(&hist_dir);
        let snap_path = hist_dir.join(format!("{}.json", current_epoch));
        let _ = std::fs::write(&snap_path, serde_json::to_vec(params).unwrap());

        let queue = self.activation_queue();
        let mut to_remove = vec![];
        for item in queue.iter() {
            let (k, v) = item?;
            let epoch: u64 = de(&k)?;
            if epoch <= current_epoch {
                let ids: Vec<u64> = de(&v).unwrap_or_else(|_| vec![]);
                for prop_id in ids {
                    let key = ser(&prop_id)?;
                    if let Some(raw) = self.proposals().get(&key)? {
                        let mut prop: Proposal = de(&raw)?;
                        if prop.status == ProposalStatus::Passed {
                            let old = match prop.key {
                                ParamKey::SnapshotIntervalSecs => params.snapshot_interval_secs,
                                ParamKey::ConsumerFeeComfortP90Microunits => {
                                    params.consumer_fee_comfort_p90_microunits
                                }
                                ParamKey::IndustrialAdmissionMinCapacity => {
                                    params.industrial_admission_min_capacity
                                }
                                ParamKey::FairshareGlobalMax => params.fairshare_global_max_ppm,
                                ParamKey::BurstRefillRatePerS => params.burst_refill_rate_per_s_ppm,
                                ParamKey::CreditsDecayLambdaPerHourPpm => {
                                    params.credits_decay_lambda_per_hour_ppm
                                }
                                ParamKey::DailyPayoutCap => params.daily_payout_cap as i64,
                            };
                            if let Some(spec) = registry().iter().find(|s| s.key == prop.key) {
                                (spec.apply)(prop.new_value, params)
                                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
                                (spec.apply_runtime)(prop.new_value, rt)
                                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
                            }
                            let last = LastActivation {
                                proposal_id: prop.id,
                                key: prop.key,
                                old_value: old,
                                new_value: prop.new_value,
                                activated_epoch: current_epoch,
                            };
                            self.last_activation().insert("last", ser(&last)?)?;
                            prop.status = ProposalStatus::Activated;
                            self.proposals().insert(&key, ser(&prop)?)?;
                            self.active_params()
                                .insert(ser(&prop.key)?, ser(&prop.new_value)?)?;
                            #[cfg(feature = "telemetry")]
                            {
                                PARAM_CHANGE_PENDING
                                    .with_label_values(&[key_name(prop.key)])
                                    .set(0);
                                PARAM_CHANGE_ACTIVE
                                    .with_label_values(&[key_name(prop.key)])
                                    .set(prop.new_value);
                                let sched = prop.activation_epoch.unwrap_or(current_epoch);
                                let delay = current_epoch.saturating_sub(sched);
                                GOV_ACTIVATION_DELAY_SECONDS
                                    .with_label_values(&[key_name(prop.key)])
                                    .observe(delay as f64);
                                governance_webhook("activate", prop.id);
                                info!(
                                    "gov_param_activated key={:?} new={} old={} epoch={}",
                                    prop.key, prop.new_value, old, current_epoch
                                );
                            }
                        }
                    }
                }
                to_remove.push(epoch);
            }
        }
        for e in to_remove {
            queue.remove(ser(&e)?)?;
        }
        Ok(())
    }

    pub fn rollback_last(
        &self,
        current_epoch: u64,
        rt: &mut Runtime,
        params: &mut Params,
    ) -> sled::Result<()> {
        if let Some(raw) = self.last_activation().get("last")? {
            let last: LastActivation = de(&raw)?;
            if current_epoch > last.activated_epoch + ROLLBACK_WINDOW_EPOCHS {
                return Err(sled::Error::Unsupported("expired".into()));
            }
            if let Some(spec) = registry().iter().find(|s| s.key == last.key) {
                (spec.apply)(last.old_value, params)
                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
                (spec.apply_runtime)(last.old_value, rt)
                    .map_err(|_| sled::Error::Unsupported("apply".into()))?;
            }
            self.active_params()
                .insert(ser(&last.key)?, ser(&last.old_value)?)?;
            if let Some(prop_raw) = self.proposals().get(ser(&last.proposal_id)?)? {
                let mut prop: Proposal = de(&prop_raw)?;
                prop.status = ProposalStatus::RolledBack;
                self.proposals().insert(ser(&prop.id)?, ser(&prop)?)?;
            }
            self.last_activation().remove("last")?;
            #[cfg(feature = "telemetry")]
            {
                PARAM_CHANGE_ACTIVE
                    .with_label_values(&[key_name(last.key)])
                    .set(last.old_value);
                GOV_ROLLBACK_TOTAL
                    .with_label_values(&[key_name(last.key)])
                    .inc();
                governance_webhook("rollback", last.proposal_id);
            }
            return Ok(());
        }
        Err(sled::Error::ReportableBug("no activation".into()))
    }

    pub fn rollback_proposal(
        &self,
        proposal_id: u64,
        current_epoch: u64,
        rt: &mut Runtime,
        params: &mut Params,
    ) -> sled::Result<()> {
        let key = ser(&proposal_id)?;
        let prop_raw = self
            .proposals()
            .get(&key)?
            .ok_or_else(|| sled::Error::Unsupported("missing proposal".into()))?;
        let mut prop: Proposal = de(&prop_raw)?;
        let act_epoch = prop
            .activation_epoch
            .ok_or_else(|| sled::Error::Unsupported("not activated".into()))?;
        if current_epoch > act_epoch + ROLLBACK_WINDOW_EPOCHS {
            return Err(sled::Error::Unsupported("expired".into()));
        }
        let snap_path = self
            .base_path
            .join("governance/history")
            .join(format!("{}.json", act_epoch));
        let bytes =
            std::fs::read(&snap_path).map_err(|_| sled::Error::Unsupported("snapshot".into()))?;
        let prev: Params =
            serde_json::from_slice(&bytes).map_err(|_| sled::Error::Unsupported("parse".into()))?;
        *params = prev.clone();
        for spec in registry() {
            let val = match spec.key {
                ParamKey::SnapshotIntervalSecs => params.snapshot_interval_secs,
                ParamKey::ConsumerFeeComfortP90Microunits => {
                    params.consumer_fee_comfort_p90_microunits
                }
                ParamKey::IndustrialAdmissionMinCapacity => {
                    params.industrial_admission_min_capacity
                }
                ParamKey::FairshareGlobalMax => params.fairshare_global_max_ppm,
                ParamKey::BurstRefillRatePerS => params.burst_refill_rate_per_s_ppm,
                ParamKey::CreditsDecayLambdaPerHourPpm => params.credits_decay_lambda_per_hour_ppm,
                ParamKey::DailyPayoutCap => params.daily_payout_cap as i64,
            };
            (spec.apply_runtime)(val, rt)
                .map_err(|_| sled::Error::Unsupported("apply_runtime".into()))?;
            self.active_params().insert(ser(&spec.key)?, ser(&val)?)?;
        }
        prop.status = ProposalStatus::RolledBack;
        self.proposals().insert(key, ser(&prop)?)?;
        #[cfg(feature = "telemetry")]
        {
            GOV_ROLLBACK_TOTAL
                .with_label_values(&[key_name(prop.key)])
                .inc();
            governance_webhook("rollback", proposal_id);
        }
        Ok(())
    }
}
