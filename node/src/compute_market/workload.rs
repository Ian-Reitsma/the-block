use foundation_serialization::{Deserialize, Serialize};

/// Normalised compute unit (e.g., GPU-seconds scaled by FLOPS).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ComputeUnits(pub u64);

/// Estimate compute units from raw workload bytes. Currently 1 unit per MiB.
pub fn compute_units(data: &[u8]) -> u64 {
    ((data.len() as u64) + 1_048_575) / 1_048_576
}

/// Calibrate hardware; returns units produced per second for a given GPU.
pub fn calibrate_gpu(gflops: u64) -> ComputeUnits {
    ComputeUnits(gflops)
}
