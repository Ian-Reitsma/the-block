use super::{registry, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote, VoteChoice};
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    governance_webhook, GOV_ACTIVATION_DELAY_SECONDS, GOV_PROPOSALS_PENDING, GOV_ROLLBACK_TOTAL,
    GOV_VOTES_TOTAL, PARAM_CHANGE_ACTIVE, PARAM_CHANGE_PENDING,
};
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
        ParamKey::BetaStorageSubCt => "beta_storage_sub_ct",
        ParamKey::GammaReadSubCt => "gamma_read_sub_ct",
        ParamKey::KappaCpuSubCt => "kappa_cpu_sub_ct",
        ParamKey::LambdaBytesOutSubCt => "lambda_bytes_out_sub_ct",
        ParamKey::RentRateCtPerByte => "rent_rate_ct_per_byte",
        ParamKey::KillSwitchSubsidyReduction => "kill_switch_subsidy_reduction",
        ParamKey::MinerRewardLogisticTarget => "miner_reward_logistic_target",
        ParamKey::LogisticSlope => "logistic_slope_milli",
        ParamKey::MinerHysteresis => "miner_hysteresis",
        ParamKey::HeuristicMuMilli => "heuristic_mu_milli",
        ParamKey::BadgeExpirySecs => "badge_expiry_secs",
        ParamKey::BadgeIssueUptime => "badge_issue_uptime_percent",
        ParamKey::BadgeRevokeUptime => "badge_revoke_uptime_percent",
        ParamKey::JurisdictionRegion => "jurisdiction_region",
        ParamKey::AiDiagnosticsEnabled => "ai_diagnostics_enabled",
        ParamKey::KalmanRShort => "kalman_r_short",
        ParamKey::KalmanRMed => "kalman_r_med",
        ParamKey::KalmanRLong => "kalman_r_long",
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

    pub fn proposals(&self) -> sled::Tree {
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
        // Ensure dependencies exist and graph remains acyclic
        for dep in &p.deps {
            if self.proposals().get(ser(dep)?)?.is_none() {
                return Err(sled::Error::Unsupported("missing dependency".into()));
            }
        }
        let next = self
            .next_id()
            .get("id")?
            .map(|v| de::<u64>(&v))
            .transpose()?
            .unwrap_or(0);
        self.next_id().insert("id", ser(&(next + 1))?)?;
        p.id = next;
        // collect existing proposals for cycle detection
        let mut existing = std::collections::HashMap::new();
        for item in self.proposals().iter() {
            let (k, v) = item?;
            let id: u64 = de(&k)?;
            let prop: Proposal = de(&v)?;
            existing.insert(id, prop);
        }
        if !super::validate_dag(&existing, &p) {
            return Err(sled::Error::Unsupported("cycle".into()));
        }
        self.proposals().insert(ser(&p.id)?, ser(&p)?)?;
        #[cfg(feature = "telemetry")]
        self.update_pending_gauge()?;
        Ok(next)
    }

    #[cfg(feature = "telemetry")]
    fn update_pending_gauge(&self) -> sled::Result<()> {
        let mut pending = 0i64;
        for item in self.proposals().iter() {
            let (_, v) = item?;
            let prop: Proposal = de(&v)?;
            if prop.status == ProposalStatus::Open || prop.status == ProposalStatus::Passed {
                pending += 1;
            }
        }
        GOV_PROPOSALS_PENDING.set(pending);
        Ok(())
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
            let spec = registry()
                .iter()
                .find(|s| s.key == prop.key)
                .expect("param spec");
            let delay = if spec.timelock_epochs > 0 {
                spec.timelock_epochs
            } else {
                ACTIVATION_DELAY
            };
            let act_epoch = current_epoch + delay;
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
        #[cfg(feature = "telemetry")]
        self.update_pending_gauge()?;
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
        if let Ok(bytes) = serde_json::to_vec(params) {
            let _ = std::fs::write(&snap_path, bytes);
        }

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
                                ParamKey::BetaStorageSubCt => params.beta_storage_sub_ct,
                                ParamKey::GammaReadSubCt => params.gamma_read_sub_ct,
                                ParamKey::KappaCpuSubCt => params.kappa_cpu_sub_ct,
                                ParamKey::LambdaBytesOutSubCt => params.lambda_bytes_out_sub_ct,
                                ParamKey::RentRateCtPerByte => params.rent_rate_ct_per_byte,
                                ParamKey::KillSwitchSubsidyReduction => {
                                    params.kill_switch_subsidy_reduction
                                }
                                ParamKey::MinerRewardLogisticTarget => {
                                    params.miner_reward_logistic_target
                                }
                                ParamKey::LogisticSlope => params.logistic_slope_milli,
                                ParamKey::MinerHysteresis => params.miner_hysteresis,
                                ParamKey::HeuristicMuMilli => params.heuristic_mu_milli,
                                ParamKey::BadgeExpirySecs => params.badge_expiry_secs,
                                ParamKey::BadgeIssueUptime => params.badge_issue_uptime_percent,
                                ParamKey::BadgeRevokeUptime => params.badge_revoke_uptime_percent,
                                ParamKey::JurisdictionRegion => params.jurisdiction_region,
                                ParamKey::AiDiagnosticsEnabled => params.ai_diagnostics_enabled,
                                ParamKey::KalmanRShort => params.kalman_r_short,
                                ParamKey::KalmanRMed => params.kalman_r_med,
                                ParamKey::KalmanRLong => params.kalman_r_long,
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
                                if crate::telemetry::should_log("governance") {
                                    let span = crate::log_context!(block = current_epoch);
                                    tracing::info!(
                                        parent: &span,
                                        "gov_param_activated key={:?} new={} old={} epoch={}",
                                        prop.key,
                                        prop.new_value,
                                        old,
                                        current_epoch
                                    );
                                }
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
        #[cfg(feature = "telemetry")]
        self.update_pending_gauge()?;
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
            #[cfg(feature = "telemetry")]
            self.update_pending_gauge()?;
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
                ParamKey::BetaStorageSubCt => params.beta_storage_sub_ct,
                ParamKey::GammaReadSubCt => params.gamma_read_sub_ct,
                ParamKey::KappaCpuSubCt => params.kappa_cpu_sub_ct,
                ParamKey::LambdaBytesOutSubCt => params.lambda_bytes_out_sub_ct,
                ParamKey::RentRateCtPerByte => params.rent_rate_ct_per_byte,
                ParamKey::KillSwitchSubsidyReduction => params.kill_switch_subsidy_reduction as i64,
                ParamKey::MinerRewardLogisticTarget => params.miner_reward_logistic_target,
                ParamKey::LogisticSlope => params.logistic_slope_milli,
                ParamKey::MinerHysteresis => params.miner_hysteresis,
                ParamKey::HeuristicMuMilli => params.heuristic_mu_milli,
                ParamKey::BadgeExpirySecs => params.badge_expiry_secs,
                ParamKey::BadgeIssueUptime => params.badge_issue_uptime_percent,
                ParamKey::BadgeRevokeUptime => params.badge_revoke_uptime_percent,
                ParamKey::JurisdictionRegion => params.jurisdiction_region,
                ParamKey::AiDiagnosticsEnabled => params.ai_diagnostics_enabled,
                ParamKey::KalmanRShort => params.kalman_r_short,
                ParamKey::KalmanRMed => params.kalman_r_med,
                ParamKey::KalmanRLong => params.kalman_r_long,
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
            self.update_pending_gauge()?;
        }
        Ok(())
    }
}
