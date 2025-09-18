use super::ParamKey;
use crate::scheduler::{self, ServiceClass};
use crate::Blockchain;
use bincode;
use serde::{Deserialize, Serialize};
use serde_json;
use std::time::Duration;
use std::{fs, fs::OpenOptions, io::Write, path::Path};
#[cfg(feature = "telemetry")]
use tracing::info;

pub struct Runtime<'a> {
    pub bc: &'a mut Blockchain,
}

impl<'a> Runtime<'a> {
    pub fn set_consumer_p90_comfort(&mut self, v: u64) {
        self.bc.set_consumer_p90_comfort(v);
    }
    pub fn set_fee_floor_policy(&mut self, window: u64, percentile: u64) {
        self.bc.params.fee_floor_window = window as i64;
        self.bc.params.fee_floor_percentile = percentile as i64;
        self.bc
            .set_fee_floor_policy(window as usize, percentile as u32);
    }
    pub fn set_min_capacity(&mut self, v: u64) {
        crate::compute_market::admission::set_min_capacity(v);
    }
    pub fn set_snapshot_interval(&mut self, d: Duration) {
        self.bc.snapshot.set_interval(d.as_secs());
    }
    pub fn set_fair_share_cap(&mut self, v: f64) {
        crate::compute_market::admission::set_fair_share_cap(v);
    }
    pub fn set_burst_refill_rate(&mut self, v: f64) {
        crate::compute_market::admission::set_burst_refill_rate(v);
    }
    pub fn set_rent_rate(&mut self, v: i64) {
        self.bc.params.rent_rate_ct_per_byte = v;
    }
    pub fn set_badge_expiry(&mut self, v: u64) {
        crate::service_badge::set_badge_ttl_secs(v);
    }
    pub fn set_badge_issue_uptime(&mut self, v: u64) {
        crate::service_badge::set_badge_issue_uptime(v);
    }
    pub fn set_badge_revoke_uptime(&mut self, v: u64) {
        crate::service_badge::set_badge_revoke_uptime(v);
    }
    pub fn set_jurisdiction_region(&mut self, v: i64) {
        let region = match v {
            1 => "US",
            2 => "EU",
            _ => "UNSPEC",
        };
        let language = default_language_for_region(region);
        self.bc.config.jurisdiction = Some(region.to_string());
        self.bc.save_config();
        let _ = crate::le_portal::record_action(
            &self.bc.path,
            "governance",
            &format!("set_jurisdiction_{region}"),
            region,
            language,
        );
    }
    pub fn set_ai_diagnostics_enabled(&mut self, v: bool) {
        self.bc.params.ai_diagnostics_enabled = v as i64;
    }
    pub fn set_scheduler_weight(&mut self, class: ServiceClass, weight: u64) {
        scheduler::set_weight(class, weight as u32);
    }
}

fn default_language_for_region(region: &str) -> &'static str {
    match region {
        "US" => "en-US",
        "EU" => "en-GB",
        "UNSPEC" => "en",
        _ => "en",
    }
}

pub struct ParamSpec {
    pub key: ParamKey,
    pub default: i64,
    pub min: i64,
    pub max: i64,
    pub unit: &'static str,
    pub timelock_epochs: u64,
    pub apply: fn(i64, &mut Params) -> Result<(), ()>,
    pub apply_runtime: fn(i64, &mut Runtime) -> Result<(), ()>,
}

const DEFAULT_TIMELOCK_EPOCHS: u64 = 2;
const KILL_SWITCH_TIMELOCK_EPOCHS: u64 = 10800; // ≈12h at 4s epochs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Params {
    pub snapshot_interval_secs: i64,
    pub consumer_fee_comfort_p90_microunits: i64,
    pub fee_floor_window: i64,
    pub fee_floor_percentile: i64,
    pub industrial_admission_min_capacity: i64,
    pub fairshare_global_max_ppm: i64,
    pub burst_refill_rate_per_s_ppm: i64,
    pub beta_storage_sub_ct: i64,
    pub gamma_read_sub_ct: i64,
    pub kappa_cpu_sub_ct: i64,
    pub lambda_bytes_out_sub_ct: i64,
    pub rent_rate_ct_per_byte: i64,
    pub kill_switch_subsidy_reduction: i64,
    pub miner_reward_logistic_target: i64,
    pub logistic_slope_milli: i64,
    pub miner_hysteresis: i64,
    pub risk_lambda: i64,
    pub entropy_phi: i64,
    pub haar_eta: i64,
    pub util_var_threshold: i64,
    pub fib_window_base_secs: i64,
    pub heuristic_mu_milli: i64,
    pub industrial_multiplier: i64,
    pub badge_expiry_secs: i64,
    pub badge_issue_uptime_percent: i64,
    pub badge_revoke_uptime_percent: i64,
    pub jurisdiction_region: i64,
    pub ai_diagnostics_enabled: i64,
    pub kalman_r_short: i64,
    pub kalman_r_med: i64,
    pub kalman_r_long: i64,
    pub scheduler_weight_gossip: i64,
    pub scheduler_weight_compute: i64,
    pub scheduler_weight_storage: i64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            snapshot_interval_secs: 30,
            consumer_fee_comfort_p90_microunits: 2_500,
            fee_floor_window: 256,
            fee_floor_percentile: 75,
            industrial_admission_min_capacity: 10,
            fairshare_global_max_ppm: 250_000,
            burst_refill_rate_per_s_ppm: ((30.0 / 60.0) * 1_000_000.0) as i64,
            beta_storage_sub_ct: 50,
            gamma_read_sub_ct: 20,
            kappa_cpu_sub_ct: 10,
            lambda_bytes_out_sub_ct: 5,
            rent_rate_ct_per_byte: 0,
            kill_switch_subsidy_reduction: 0,
            miner_reward_logistic_target: 100,
            logistic_slope_milli: (99f64.ln() / (0.1 * 100.0) * 1000.0) as i64,
            miner_hysteresis: 10,
            risk_lambda: (4.0 * std::f64::consts::LN_2 * 1000.0) as i64,
            entropy_phi: 2000,
            haar_eta: 1500,
            util_var_threshold: 100,
            fib_window_base_secs: 4,
            heuristic_mu_milli: 500,
            industrial_multiplier: 100,
            badge_expiry_secs: 30 * 24 * 60 * 60,
            badge_issue_uptime_percent: 99,
            badge_revoke_uptime_percent: 95,
            jurisdiction_region: 0,
            ai_diagnostics_enabled: 0,
            kalman_r_short: 1,
            kalman_r_med: 3,
            kalman_r_long: 8,
            scheduler_weight_gossip: 3,
            scheduler_weight_compute: 2,
            scheduler_weight_storage: 1,
        }
    }
}

fn apply_snapshot_interval(v: i64, p: &mut Params) -> Result<(), ()> {
    p.snapshot_interval_secs = v;
    Ok(())
}
fn apply_consumer_fee_p90(v: i64, p: &mut Params) -> Result<(), ()> {
    p.consumer_fee_comfort_p90_microunits = v;
    Ok(())
}
fn apply_fee_floor_window(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 1 {
        return Err(());
    }
    p.fee_floor_window = v;
    Ok(())
}
fn apply_fee_floor_percentile(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 || v > 100 {
        return Err(());
    }
    p.fee_floor_percentile = v;
    Ok(())
}
fn apply_industrial_capacity(v: i64, p: &mut Params) -> Result<(), ()> {
    p.industrial_admission_min_capacity = v;
    Ok(())
}

fn apply_badge_expiry(v: i64, p: &mut Params) -> Result<(), ()> {
    p.badge_expiry_secs = v;
    Ok(())
}
fn apply_badge_issue_uptime(v: i64, p: &mut Params) -> Result<(), ()> {
    p.badge_issue_uptime_percent = v;
    Ok(())
}
fn apply_badge_revoke_uptime(v: i64, p: &mut Params) -> Result<(), ()> {
    p.badge_revoke_uptime_percent = v;
    Ok(())
}
fn apply_jurisdiction_region(v: i64, p: &mut Params) -> Result<(), ()> {
    p.jurisdiction_region = v;
    Ok(())
}
fn apply_ai_diagnostics_enabled(v: i64, p: &mut Params) -> Result<(), ()> {
    p.ai_diagnostics_enabled = v;
    Ok(())
}

fn apply_kalman_r_short(v: i64, p: &mut Params) -> Result<(), ()> {
    p.kalman_r_short = v;
    Ok(())
}

fn apply_kalman_r_med(v: i64, p: &mut Params) -> Result<(), ()> {
    p.kalman_r_med = v;
    Ok(())
}

fn apply_kalman_r_long(v: i64, p: &mut Params) -> Result<(), ()> {
    p.kalman_r_long = v;
    Ok(())
}

fn apply_scheduler_weight_gossip(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.scheduler_weight_gossip = v;
    Ok(())
}

fn apply_scheduler_weight_compute(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.scheduler_weight_compute = v;
    Ok(())
}

fn apply_scheduler_weight_storage(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.scheduler_weight_storage = v;
    Ok(())
}
fn apply_fairshare_global_max(v: i64, p: &mut Params) -> Result<(), ()> {
    p.fairshare_global_max_ppm = v;
    Ok(())
}
fn apply_burst_refill_rate(v: i64, p: &mut Params) -> Result<(), ()> {
    p.burst_refill_rate_per_s_ppm = v;
    Ok(())
}

fn apply_beta_storage_sub(v: i64, p: &mut Params) -> Result<(), ()> {
    p.beta_storage_sub_ct = v;
    Ok(())
}

fn apply_gamma_read_sub(v: i64, p: &mut Params) -> Result<(), ()> {
    p.gamma_read_sub_ct = v;
    Ok(())
}

fn apply_kappa_cpu_sub(v: i64, p: &mut Params) -> Result<(), ()> {
    p.kappa_cpu_sub_ct = v;
    Ok(())
}

fn apply_lambda_bytes_out_sub(v: i64, p: &mut Params) -> Result<(), ()> {
    p.lambda_bytes_out_sub_ct = v;
    Ok(())
}

fn apply_rent_rate(v: i64, p: &mut Params) -> Result<(), ()> {
    p.rent_rate_ct_per_byte = v;
    Ok(())
}

fn apply_kill_switch(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 || v > 100 {
        return Err(());
    }
    p.kill_switch_subsidy_reduction = v;
    Ok(())
}

fn apply_miner_target(v: i64, p: &mut Params) -> Result<(), ()> {
    if v <= 0 {
        return Err(());
    }
    p.miner_reward_logistic_target = v;
    Ok(())
}

fn apply_logistic_slope(v: i64, p: &mut Params) -> Result<(), ()> {
    if v <= 0 {
        return Err(());
    }
    p.logistic_slope_milli = v;
    Ok(())
}

fn apply_miner_hysteresis(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.miner_hysteresis = v;
    Ok(())
}

fn apply_heuristic_mu(v: i64, p: &mut Params) -> Result<(), ()> {
    if v < 0 {
        return Err(());
    }
    p.heuristic_mu_milli = v;
    #[cfg(feature = "telemetry")]
    crate::telemetry::HEURISTIC_MU_MILLI.set(v);
    Ok(())
}

pub fn registry() -> &'static [ParamSpec] {
    static REGS: [ParamSpec; 25] = [
        ParamSpec {
            key: ParamKey::SnapshotIntervalSecs,
            default: 30,
            min: 5,
            max: 600,
            unit: "seconds",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_snapshot_interval,
            apply_runtime: |v, rt| {
                rt.set_snapshot_interval(Duration::from_secs(v as u64));
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::ConsumerFeeComfortP90Microunits,
            default: 2_500,
            min: 500,
            max: 25_000,
            unit: "microunits",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_consumer_fee_p90,
            apply_runtime: |v, rt| {
                rt.set_consumer_p90_comfort(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::FeeFloorWindow,
            default: 256,
            min: 1,
            max: 4_096,
            unit: "samples",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_fee_floor_window,
            apply_runtime: |v, rt| {
                let percentile = rt.bc.params.fee_floor_percentile as u64;
                rt.set_fee_floor_policy(v as u64, percentile);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::FeeFloorPercentile,
            default: 75,
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_fee_floor_percentile,
            apply_runtime: |v, rt| {
                let window = rt.bc.params.fee_floor_window as u64;
                rt.set_fee_floor_policy(window, v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::IndustrialAdmissionMinCapacity,
            default: 10,
            min: 1,
            max: 10_000,
            unit: "microshards/sec",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_industrial_capacity,
            apply_runtime: |v, rt| {
                rt.set_min_capacity(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::FairshareGlobalMax,
            default: 250_000,
            min: 10_000,
            max: 1_000_000,
            unit: "ppm",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_fairshare_global_max,
            apply_runtime: |v, rt| {
                rt.set_fair_share_cap(v as f64 / 1_000_000.0);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::BurstRefillRatePerS,
            default: ((30.0 / 60.0) * 1_000_000.0) as i64,
            min: 0,
            max: 1_000_000,
            unit: "ppm",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_burst_refill_rate,
            apply_runtime: |v, rt| {
                rt.set_burst_refill_rate(v as f64 / 1_000_000.0);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::BetaStorageSubCt,
            default: 50,
            min: 0,
            max: 1_000_000,
            unit: "nCT per byte",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_beta_storage_sub,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::GammaReadSubCt,
            default: 20,
            min: 0,
            max: 1_000_000,
            unit: "nCT per byte",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_gamma_read_sub,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::KappaCpuSubCt,
            default: 10,
            min: 0,
            max: 1_000_000,
            unit: "nCT per ms",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_kappa_cpu_sub,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::LambdaBytesOutSubCt,
            default: 5,
            min: 0,
            max: 1_000_000,
            unit: "nCT per byte",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_lambda_bytes_out_sub,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::RentRateCtPerByte,
            default: 0,
            min: 0,
            max: 1_000_000,
            unit: "nCT per byte",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_rent_rate,
            apply_runtime: |v, rt| {
                rt.set_rent_rate(v);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::KillSwitchSubsidyReduction,
            default: 0,
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: KILL_SWITCH_TIMELOCK_EPOCHS,
            apply: apply_kill_switch,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::MinerRewardLogisticTarget,
            default: 100,
            min: 1,
            max: 1_000_000,
            unit: "miners",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_miner_target,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::LogisticSlope,
            default: 460,
            min: 1,
            max: 1_000_000,
            unit: "slope_milli",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_logistic_slope,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::MinerHysteresis,
            default: 10,
            min: 0,
            max: 1_000_000,
            unit: "miners",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_miner_hysteresis,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::HeuristicMuMilli,
            default: 500,
            min: 0,
            max: 10_000,
            unit: "milli",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_heuristic_mu,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::BadgeExpirySecs,
            default: 30 * 24 * 60 * 60,
            min: 3_600,
            max: 365 * 24 * 60 * 60,
            unit: "seconds",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_badge_expiry,
            apply_runtime: |v, rt| {
                rt.set_badge_expiry(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::BadgeIssueUptime,
            default: 99,
            min: 50,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_badge_issue_uptime,
            apply_runtime: |v, rt| {
                rt.set_badge_issue_uptime(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::BadgeRevokeUptime,
            default: 95,
            min: 0,
            max: 100,
            unit: "percent",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_badge_revoke_uptime,
            apply_runtime: |v, rt| {
                rt.set_badge_revoke_uptime(v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::JurisdictionRegion,
            default: 0,
            min: 0,
            max: 10,
            unit: "code",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_jurisdiction_region,
            apply_runtime: |v, rt| {
                rt.set_jurisdiction_region(v);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::AiDiagnosticsEnabled,
            default: 0,
            min: 0,
            max: 1,
            unit: "bool",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_ai_diagnostics_enabled,
            apply_runtime: |v, rt| {
                rt.set_ai_diagnostics_enabled(v != 0);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::KalmanRShort,
            default: 1,
            min: 1,
            max: 1_000,
            unit: "weight",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_kalman_r_short,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::KalmanRMed,
            default: 3,
            min: 1,
            max: 1_000,
            unit: "weight",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_kalman_r_med,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::KalmanRLong,
            default: 8,
            min: 1,
            max: 1_000,
            unit: "weight",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_kalman_r_long,
            apply_runtime: |_v, _rt| Ok(()),
        },
        ParamSpec {
            key: ParamKey::SchedulerWeightGossip,
            default: 3,
            min: 0,
            max: 16,
            unit: "tickets",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_scheduler_weight_gossip,
            apply_runtime: |v, rt| {
                rt.set_scheduler_weight(ServiceClass::Gossip, v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::SchedulerWeightCompute,
            default: 2,
            min: 0,
            max: 16,
            unit: "tickets",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_scheduler_weight_compute,
            apply_runtime: |v, rt| {
                rt.set_scheduler_weight(ServiceClass::Compute, v as u64);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::SchedulerWeightStorage,
            default: 1,
            min: 0,
            max: 16,
            unit: "tickets",
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
            apply: apply_scheduler_weight_storage,
            apply_runtime: |v, rt| {
                rt.set_scheduler_weight(ServiceClass::Storage, v as u64);
                Ok(())
            },
        },
    ];
    &REGS
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Utilization {
    pub bytes_stored: f64,
    pub bytes_read: f64,
    pub cpu_ms: f64,
    pub bytes_out: f64,
    pub epoch_secs: f64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct EncryptedUtilization(pub Vec<u8>);

#[allow(dead_code)]
impl EncryptedUtilization {
    pub fn decrypt(&self, key: &[u8]) -> Utilization {
        let mut buf = self.0.clone();
        for (b, k) in buf.iter_mut().zip(key.iter().cycle()) {
            *b ^= k;
        }
        bincode::deserialize(&buf).unwrap_or_default()
    }
}

pub fn retune_multipliers(
    params: &mut Params,
    supply: f64,
    stats: &Utilization,
    current_epoch: u64,
    base_path: &Path,
    rolling_inflation: f64,
    rng_seed: Option<u64>,
) -> [i64; 4] {
    #[derive(Serialize, Deserialize)]
    struct KalmanState {
        x: [f64; 8],
        p: Vec<f64>,
    }
    #[derive(Serialize, Deserialize, Default)]
    struct UtilHistory {
        bytes_stored: Vec<f64>,
        bytes_read: Vec<f64>,
        cpu_ms: Vec<f64>,
        bytes_out: Vec<f64>,
    }

    let target = 0.02_f64;
    let hist_dir = base_path.join("governance/history");
    let _ = fs::create_dir_all(&hist_dir);
    let state_path = hist_dir.join("kalman_state.json");

    // Load previous Kalman filter state or initialise from current params.
    let mut state: KalmanState = if let Ok(bytes) = fs::read(&state_path) {
        serde_json::from_slice(&bytes).unwrap_or(KalmanState {
            x: [
                params.beta_storage_sub_ct as f64,
                params.gamma_read_sub_ct as f64,
                params.kappa_cpu_sub_ct as f64,
                params.lambda_bytes_out_sub_ct as f64,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            p: vec![0.0; 64],
        })
    } else {
        KalmanState {
            x: [
                params.beta_storage_sub_ct as f64,
                params.gamma_read_sub_ct as f64,
                params.kappa_cpu_sub_ct as f64,
                params.lambda_bytes_out_sub_ct as f64,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            p: vec![0.0; 64],
        }
    };

    // Load utilization history and append current stats.
    const MAX_HIST: usize = 256;
    let hist_path = hist_dir.join("util_history.json");
    let mut hist: UtilHistory = if let Ok(bytes) = fs::read(&hist_path) {
        serde_json::from_slice(&bytes).unwrap_or_default()
    } else {
        UtilHistory::default()
    };
    hist.bytes_stored.push(stats.bytes_stored);
    hist.bytes_read.push(stats.bytes_read);
    hist.cpu_ms.push(stats.cpu_ms);
    hist.bytes_out.push(stats.bytes_out);
    if hist.bytes_stored.len() > MAX_HIST {
        hist.bytes_stored.remove(0);
    }
    if hist.bytes_read.len() > MAX_HIST {
        hist.bytes_read.remove(0);
    }
    if hist.cpu_ms.len() > MAX_HIST {
        hist.cpu_ms.remove(0);
    }
    if hist.bytes_out.len() > MAX_HIST {
        hist.bytes_out.remove(0);
    }

    fn smooth(data: &[f64], current: f64, base: usize, eta: f64) -> f64 {
        const PHI: f64 = 1.618_033_988_749_894_8;
        let len = data.len();
        if len == 0 {
            return current;
        }
        let mut d = base.max(1);
        loop {
            let start = len.saturating_sub(d);
            let slice = &data[start..];
            // Hampel filter: compute median and MAD, reject outliers beyond 3*MAD
            let mut sorted = slice.to_vec();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let median = sorted[sorted.len() / 2];
            let mut devs: Vec<f64> = slice.iter().map(|v| (v - median).abs()).collect();
            devs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mad = devs[devs.len() / 2].max(1e-9);
            let thresh = 3.0 * mad;
            let filtered: Vec<f64> = slice
                .iter()
                .cloned()
                .filter(|v| (v - median).abs() <= thresh)
                .collect();
            let mean = filtered.iter().sum::<f64>() / filtered.len() as f64;
            let var = if filtered.len() > 1 {
                filtered.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / filtered.len() as f64
            } else {
                0.0
            };
            if var <= eta * eta * current * current || d >= len {
                return mean;
            }
            d = (PHI * d as f64).ceil() as usize;
            if d > len {
                d = len;
            }
        }
    }

    let base_epochs = (params.fib_window_base_secs as f64 / stats.epoch_secs).ceil() as usize;
    let eta = params.util_var_threshold as f64 / 1000.0;
    let smoothed = [
        smooth(&hist.bytes_stored, stats.bytes_stored, base_epochs, eta),
        smooth(&hist.bytes_read, stats.bytes_read, base_epochs, eta),
        smooth(&hist.cpu_ms, stats.cpu_ms, base_epochs, eta),
        smooth(&hist.bytes_out, stats.bytes_out, base_epochs, eta),
    ];

    let eta = params.haar_eta as f64 / 1000.0;
    let burst = crate::governance::variance::haar_burst_veto(&hist.bytes_stored, eta);

    let measurements = [
        if smoothed[0] <= 0.0 {
            state.x[0] * 2.0
        } else {
            (0.004 * target * supply / 365.0) / (smoothed[0] / stats.epoch_secs)
        },
        if smoothed[1] <= 0.0 {
            state.x[1] * 2.0
        } else {
            (0.0025 * target * supply / 365.0) / (smoothed[1] / stats.epoch_secs)
        },
        if smoothed[2] <= 0.0 {
            state.x[2] * 2.0
        } else {
            (0.0025 * target * supply / 365.0) / (smoothed[2] / stats.epoch_secs)
        },
        if smoothed[3] <= 0.0 {
            state.x[3] * 2.0
        } else {
            (0.0025 * target * supply / 365.0) / (smoothed[3] / stats.epoch_secs)
        },
    ];

    use crate::governance::kalman::KalmanLqg;
    let mut kf = KalmanLqg {
        x: nalgebra::DVector::from_row_slice(&state.x),
        p: nalgebra::DMatrix::from_row_slice(8, 8, &state.p),
    };
    if !burst {
        kf.step(
            &measurements,
            stats.epoch_secs,
            params.risk_lambda as f64 / 1000.0,
        );
    }
    state.x.copy_from_slice(kf.x.as_slice());
    state.p.copy_from_slice(kf.p.as_slice());
    let theta = kf.theta();
    let raw = [
        theta[0].round() as i64,
        theta[1].round() as i64,
        theta[2].round() as i64,
        theta[3].round() as i64,
    ];
    use rand::{rngs::StdRng, Rng, SeedableRng};
    let b = supply * (1.0 / (1u64 << 20) as f64);
    let mut rng: StdRng = match rng_seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_rng(rand::thread_rng()).expect("rng seed"),
    };
    let noisy: [i64; 4] = raw.map(|v| {
        let u: f64 = rng.gen::<f64>() - 0.5;
        let noise = if u >= 0.0 {
            -b * (1.0 - 2.0 * u).ln()
        } else {
            b * (1.0 + 2.0 * u).ln()
        };
        (v as f64 + noise).round() as i64
    });
    params.beta_storage_sub_ct = noisy[0];
    params.gamma_read_sub_ct = noisy[1];
    params.kappa_cpu_sub_ct = noisy[2];
    params.lambda_bytes_out_sub_ct = noisy[3];

    let _ = serde_json::to_vec(&state).map(|bytes| fs::write(&state_path, bytes));
    let _ = serde_json::to_vec(&hist).map(|bytes| fs::write(&hist_path, bytes));
    let events_path = hist_dir.join("events.log");
    if rolling_inflation > 0.02 {
        #[cfg(feature = "telemetry")]
        {
            info!(
                "inflation_guard triggered (rolling_inflation={})",
                rolling_inflation
            );
            crate::telemetry::SUBSIDY_AUTO_REDUCED_TOTAL.inc();
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
        {
            let _ = writeln!(
                f,
                "{} inflation_guard {:.6}",
                current_epoch, rolling_inflation
            );
        }
        params.beta_storage_sub_ct = (params.beta_storage_sub_ct as f64 * 0.95).round() as i64;
        params.gamma_read_sub_ct = (params.gamma_read_sub_ct as f64 * 0.95).round() as i64;
        params.kappa_cpu_sub_ct = (params.kappa_cpu_sub_ct as f64 * 0.95).round() as i64;
        params.lambda_bytes_out_sub_ct =
            (params.lambda_bytes_out_sub_ct as f64 * 0.95).round() as i64;
    }
    if params.kill_switch_subsidy_reduction > 0 {
        #[cfg(feature = "telemetry")]
        {
            info!(
                "kill_switch_active reduction={}",
                params.kill_switch_subsidy_reduction
            );
            crate::telemetry::KILL_SWITCH_TRIGGER_TOTAL.inc();
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
        {
            let _ = writeln!(
                f,
                "{} kill_switch {}",
                current_epoch, params.kill_switch_subsidy_reduction
            );
        }
        let factor = 1.0 - (params.kill_switch_subsidy_reduction as f64 / 100.0);
        params.beta_storage_sub_ct = (params.beta_storage_sub_ct as f64 * factor).round() as i64;
        params.gamma_read_sub_ct = (params.gamma_read_sub_ct as f64 * factor).round() as i64;
        params.kappa_cpu_sub_ct = (params.kappa_cpu_sub_ct as f64 * factor).round() as i64;
        params.lambda_bytes_out_sub_ct =
            (params.lambda_bytes_out_sub_ct as f64 * factor).round() as i64;
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)
    {
        let _ = writeln!(
            f,
            "{} retune {} {} {} {}",
            current_epoch,
            params.beta_storage_sub_ct,
            params.gamma_read_sub_ct,
            params.kappa_cpu_sub_ct,
            params.lambda_bytes_out_sub_ct,
        );
    }
    let snap_path = hist_dir.join(format!("inflation_{}.json", current_epoch));
    if let Ok(bytes) = serde_json::to_vec(params) {
        let _ = fs::write(snap_path, bytes);
    }

    #[cfg(feature = "telemetry")]
    {
        use crate::telemetry::{SUBSIDY_MULTIPLIER, SUBSIDY_MULTIPLIER_RAW};
        SUBSIDY_MULTIPLIER
            .with_label_values(&["storage"])
            .set(params.beta_storage_sub_ct);
        SUBSIDY_MULTIPLIER
            .with_label_values(&["read"])
            .set(params.gamma_read_sub_ct);
        SUBSIDY_MULTIPLIER
            .with_label_values(&["cpu"])
            .set(params.kappa_cpu_sub_ct);
        SUBSIDY_MULTIPLIER
            .with_label_values(&["bytes_out"])
            .set(params.lambda_bytes_out_sub_ct);
        SUBSIDY_MULTIPLIER_RAW
            .with_label_values(&["storage"])
            .set(raw[0]);
        SUBSIDY_MULTIPLIER_RAW
            .with_label_values(&["read"])
            .set(raw[1]);
        SUBSIDY_MULTIPLIER_RAW
            .with_label_values(&["cpu"])
            .set(raw[2]);
        SUBSIDY_MULTIPLIER_RAW
            .with_label_values(&["bytes_out"])
            .set(raw[3]);
    }
    raw
}

#[allow(dead_code)]
pub fn retune_multipliers_encrypted(
    params: &mut Params,
    supply: f64,
    enc: &EncryptedUtilization,
    key: &[u8],
    current_epoch: u64,
    base_path: &Path,
    rolling_inflation: f64,
    rng_seed: Option<u64>,
) -> [i64; 4] {
    let stats = enc.decrypt(key);
    retune_multipliers(
        params,
        supply,
        &stats,
        current_epoch,
        base_path,
        rolling_inflation,
        rng_seed,
    )
}
