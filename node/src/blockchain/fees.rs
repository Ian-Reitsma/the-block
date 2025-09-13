use crate::fees::next_base_fee;

/// Compute the base fee for the next block given the previous
/// base fee and observed gas usage.
pub fn compute(prev: u64, gas_used: u64) -> u64 {
    next_base_fee(prev, gas_used)
}
