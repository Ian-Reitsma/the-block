#[cfg(feature = "telemetry")]
pub mod policy;

/// Target gas used per block when adjusting the base fee.
///
/// This value intentionally mirrors Ethereum's `targetGasUsed` and keeps
/// block fullness around 50 % under equilibrium.
pub const TARGET_GAS_PER_BLOCK: u64 = 1_000_000;

/// Compute the next base fee given the previous base fee and gas used.
///
/// Implements the core EIP‑1559 adjustment where the base fee is increased or
/// decreased proportional to how full the previous block was relative to the
/// target.  The update uses integer math and clamps the result to a minimum of
/// 1 to avoid a zero fee floor.
pub fn next_base_fee(prev: u64, gas_used: u64) -> u64 {
    if prev == 0 {
        return 1;
    }
    let target = TARGET_GAS_PER_BLOCK as i64;
    let used = gas_used as i64;
    let delta = prev as i64 * (used - target) / target / 8; // 12.5% cap
    let mut next = prev as i64 + delta;
    if next < 1 {
        next = 1;
    }
    next as u64
}
