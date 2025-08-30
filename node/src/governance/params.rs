use super::ParamKey;
use crate::Blockchain;
use std::time::Duration;

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
    pub fn set_credit_decay_lambda(&mut self, v: f64) {
        crate::compute_market::settlement::Settlement::set_decay_lambda(v);
    }
}

pub struct ParamSpec {
    pub key: ParamKey,
    pub default: i64,
    pub min: i64,
    pub max: i64,
    pub unit: &'static str,
    pub apply: fn(i64, &mut Params) -> Result<(), ()>,
    pub apply_runtime: fn(i64, &mut Runtime) -> Result<(), ()>,
}

#[derive(Debug, Clone)]
pub struct Params {
    pub snapshot_interval_secs: i64,
    pub consumer_fee_comfort_p90_microunits: i64,
    pub industrial_admission_min_capacity: i64,
    pub fairshare_global_max_ppm: i64,
    pub burst_refill_rate_per_s_ppm: i64,
    pub credits_decay_lambda_per_hour_ppm: i64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            snapshot_interval_secs: 30,
            consumer_fee_comfort_p90_microunits: 2_500,
            industrial_admission_min_capacity: 10,
            fairshare_global_max_ppm: 250_000,
            burst_refill_rate_per_s_ppm: ((30.0 / 60.0) * 1_000_000.0) as i64,
            credits_decay_lambda_per_hour_ppm: 0,
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
fn apply_credit_decay_lambda(v: i64, p: &mut Params) -> Result<(), ()> {
    p.credits_decay_lambda_per_hour_ppm = v;
    Ok(())
}

pub fn registry() -> &'static [ParamSpec] {
    static REGS: [ParamSpec; 6] = [
        ParamSpec {
            key: ParamKey::SnapshotIntervalSecs,
            default: 30,
            min: 5,
            max: 600,
            unit: "seconds",
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
            apply: apply_burst_refill_rate,
            apply_runtime: |v, rt| {
                rt.set_burst_refill_rate(v as f64 / 1_000_000.0);
                Ok(())
            },
        },
        ParamSpec {
            key: ParamKey::CreditsDecayLambdaPerHourPpm,
            default: 0,
            min: 0,
            max: 1_000_000,
            unit: "ppm",
            apply: apply_credit_decay_lambda,
            apply_runtime: |v, rt| {
                rt.set_credit_decay_lambda(v as f64 / 1_000_000.0);
                Ok(())
            },
        },
    ];
    &REGS
}
