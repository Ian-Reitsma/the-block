use super::ParamKey;

pub struct ParamSpec {
    pub key: ParamKey,
    pub default: i64,
    pub min: i64,
    pub max: i64,
    pub unit: &'static str,
    pub apply: fn(i64, &mut Params) -> Result<(), ()>,
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
            consumer_fee_comfort_p90_microunits: 1_000,
            industrial_admission_min_capacity: 0,
        }
    }
}

fn apply_snapshot_interval(v: i64, p: &mut Params) -> Result<(), ()> {
    p.snapshot_interval_secs = v; Ok(())
}
fn apply_consumer_fee_p90(v: i64, p: &mut Params) -> Result<(), ()> {
    p.consumer_fee_comfort_p90_microunits = v; Ok(())
}
fn apply_industrial_capacity(v: i64, p: &mut Params) -> Result<(), ()> {
    p.industrial_admission_min_capacity = v; Ok(())
}

pub fn registry() -> &'static [ParamSpec] {
    static REGS: [ParamSpec; 3] = [
        ParamSpec { key: ParamKey::SnapshotIntervalSecs, default: 30, min: 5, max: 600, unit: "secs", apply: apply_snapshot_interval },
        ParamSpec { key: ParamKey::ConsumerFeeComfortP90Microunits, default: 1_000, min: 0, max: 1_000_000_000, unit: "micro", apply: apply_consumer_fee_p90 },
        ParamSpec { key: ParamKey::IndustrialAdmissionMinCapacity, default: 0, min: 0, max: 1_000_000, unit: "shards_per_sec", apply: apply_industrial_capacity },
    ];
    &REGS
}

