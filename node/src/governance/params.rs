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
}

impl Default for Params {
    fn default() -> Self {
        Self {
            snapshot_interval_secs: 30,
            consumer_fee_comfort_p90_microunits: 2_500,
            industrial_admission_min_capacity: 10,
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

pub fn registry() -> &'static [ParamSpec] {
    static REGS: [ParamSpec; 3] = [
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
    ];
    &REGS
}
