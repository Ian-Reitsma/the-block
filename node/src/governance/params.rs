use super::ParamKey;
use crate::Blockchain;
use serde::{Deserialize, Serialize};
use serde_json;
#[cfg(feature = "telemetry")]
use tracing::info;
use std::time::Duration;
use std::{fs, fs::OpenOptions, io::Write, path::Path};

pub struct Runtime<'a> {
    pub bc: &'a mut Blockchain,
}

impl<'a> Runtime<'a> {
    pub fn set_consumer_p90_comfort(&mut self, v: u64) {
        self.bc.set_consumer_p90_comfort(v);
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
    pub industrial_admission_min_capacity: i64,
    pub fairshare_global_max_ppm: i64,
    pub burst_refill_rate_per_s_ppm: i64,
    pub beta_storage_sub_ct: i64,
    pub gamma_read_sub_ct: i64,
    pub kappa_cpu_sub_ct: i64,
    pub lambda_bytes_out_sub_ct: i64,
    pub rent_rate_ct_per_byte: i64,
    pub kill_switch_subsidy_reduction: i64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            snapshot_interval_secs: 30,
            consumer_fee_comfort_p90_microunits: 2_500,
            industrial_admission_min_capacity: 10,
            fairshare_global_max_ppm: 250_000,
            burst_refill_rate_per_s_ppm: ((30.0 / 60.0) * 1_000_000.0) as i64,
            beta_storage_sub_ct: 50,
            gamma_read_sub_ct: 20,
            kappa_cpu_sub_ct: 10,
            lambda_bytes_out_sub_ct: 5,
            rent_rate_ct_per_byte: 0,
            kill_switch_subsidy_reduction: 0,
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
fn apply_industrial_capacity(v: i64, p: &mut Params) -> Result<(), ()> {
    p.industrial_admission_min_capacity = v;
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

pub fn registry() -> &'static [ParamSpec] {
    static REGS: [ParamSpec; 11] = [
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
    ];
    &REGS
}

#[derive(Clone, Default)]
pub struct Utilization {
    pub bytes_stored: f64,
    pub bytes_read: f64,
    pub cpu_ms: f64,
    pub bytes_out: f64,
    pub epoch_secs: f64,
}

pub fn retune_multipliers(
    params: &mut Params,
    supply: f64,
    stats: &Utilization,
    current_epoch: u64,
    base_path: &Path,
    rolling_inflation: f64,
) {
    let target = 0.02_f64;
    let calc = |util: f64, phi: f64, cur: &mut i64| {
        let mut next = if util <= 0.0 {
            (*cur as f64) * 2.0
        } else {
            (phi * target * supply / 365.0) / (util / stats.epoch_secs)
        };
        let min = (*cur as f64) * 0.85;
        let max = (*cur as f64) * 1.15;
        if next < min {
            next = min;
        }
        if next > max {
            next = max;
        }
        *cur = next.round() as i64;
    };

    calc(stats.bytes_stored, 0.004, &mut params.beta_storage_sub_ct);
    calc(stats.bytes_read, 0.0025, &mut params.gamma_read_sub_ct);
    calc(stats.cpu_ms, 0.0025, &mut params.kappa_cpu_sub_ct);
    calc(stats.bytes_out, 0.0025, &mut params.lambda_bytes_out_sub_ct);
    let hist_dir = base_path.join("governance/history");
    let _ = fs::create_dir_all(&hist_dir);
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
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&events_path) {
            let _ = writeln!(f, "{} inflation_guard {:.6}", current_epoch, rolling_inflation);
        }
        params.beta_storage_sub_ct = (params.beta_storage_sub_ct as f64 * 0.95).round() as i64;
        params.gamma_read_sub_ct = (params.gamma_read_sub_ct as f64 * 0.95).round() as i64;
        params.kappa_cpu_sub_ct = (params.kappa_cpu_sub_ct as f64 * 0.95).round() as i64;
        params.lambda_bytes_out_sub_ct = (params.lambda_bytes_out_sub_ct as f64 * 0.95).round() as i64;
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
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&events_path) {
            let _ = writeln!(f, "{} kill_switch {}", current_epoch, params.kill_switch_subsidy_reduction);
        }
        let factor = 1.0 - (params.kill_switch_subsidy_reduction as f64 / 100.0);
        params.beta_storage_sub_ct = (params.beta_storage_sub_ct as f64 * factor).round() as i64;
        params.gamma_read_sub_ct = (params.gamma_read_sub_ct as f64 * factor).round() as i64;
        params.kappa_cpu_sub_ct = (params.kappa_cpu_sub_ct as f64 * factor).round() as i64;
        params.lambda_bytes_out_sub_ct = (params.lambda_bytes_out_sub_ct as f64 * factor).round() as i64;
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&events_path) {
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
        use crate::telemetry::SUBSIDY_MULTIPLIER;
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
    }
}
