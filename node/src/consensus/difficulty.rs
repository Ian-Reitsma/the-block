use super::constants::{DIFFICULTY_WINDOW, TARGET_SPACING_MS};
use crate::consensus::difficulty_retune;
use crate::governance::Params;
use crate::Block;

/// Retarget helper maintained for legacy callers.
///
/// `timestamps` should contain monotonically increasing millisecond values. The
/// optional `target_spacing_ms` parameter allows historical tests to exercise
/// alternative spacing without re-deriving the Kalman schedule. When a
/// different spacing is provided we scale the result proportionally to the
/// canonical `TARGET_SPACING_MS` so callers continue to receive sensible
/// adjustments.
pub fn retarget(prev: u64, timestamps: &[u64], target_spacing_ms: u64) -> u64 {
    if timestamps.len() < 2 {
        return prev.max(1);
    }
    let params = Params::default();
    let (next, _) = difficulty_retune::retune(prev, timestamps, 0, &params);
    if target_spacing_ms == TARGET_SPACING_MS || target_spacing_ms == 0 {
        next
    } else {
        let scale = TARGET_SPACING_MS as f64 / target_spacing_ms as f64;
        ((next as f64) * scale).round().max(1.0) as u64
    }
}

/// Compatibility shim delegating to [`difficulty_retune::retune`].
pub fn expected_difficulty(prev: u64, recent_timestamps: &[u64]) -> u64 {
    let params = Params::default();
    difficulty_retune::retune(prev, recent_timestamps, 0, &params).0
}

/// Compute expected difficulty from a chain slice using stored hints.
pub fn expected_difficulty_from_chain(chain: &[Block]) -> u64 {
    if let Some(last) = chain.last() {
        let mut ts: Vec<u64> = chain
            .iter()
            .rev()
            .take(DIFFICULTY_WINDOW)
            .map(|b| b.timestamp_millis)
            .collect();
        ts.reverse();
        let hint = chain.last().map_or(0, |b| b.retune_hint);
        difficulty_retune::retune(last.difficulty, &ts, hint, &Params::default()).0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TokenAmount;

    #[test]
    fn retarget_adjusts_up() {
        let prev = 1000;
        // Blocks coming twice as fast (60s vs target 120s)
        let next = retarget(prev, &[0, 60_000], 120_000);
        assert!(next > prev);
    }

    #[test]
    fn retarget_adjusts_down() {
        let prev = 1000;
        // Blocks coming twice as slow (240s vs target 120s)
        let next = retarget(prev, &[0, 240_000], 120_000);
        assert!(next < prev);
    }

    #[test]
    fn expected_matches_retarget_window() {
        let mut chain = Vec::new();
        let mut ts = 1u64;
        for i in 0..DIFFICULTY_WINDOW {
            chain.push(crate::Block {
                index: i as u64,
                previous_hash: String::new(),
                timestamp_millis: ts,
                transactions: Vec::new(),
                difficulty: 1000,
                retune_hint: 0,
                nonce: 0,
                hash: String::new(),
                coinbase_consumer: TokenAmount::new(0),
                coinbase_industrial: TokenAmount::new(0),
                storage_sub_ct: TokenAmount::new(0),
                read_sub_ct: TokenAmount::new(0),
                compute_sub_ct: TokenAmount::new(0),
                proof_rebate_ct: TokenAmount::new(0),
                storage_sub_it: TokenAmount::new(0),
                read_sub_it: TokenAmount::new(0),
                compute_sub_it: TokenAmount::new(0),
                read_root: [0u8; 32],
                fee_checksum: String::new(),
                state_root: String::new(),
                base_fee: 1,
                l2_roots: Vec::new(),
                l2_sizes: Vec::new(),
                vdf_commit: [0u8; 32],
                vdf_output: [0u8; 32],
                vdf_proof: Vec::new(),
            });
            ts += 2_000; // twice the target spacing
        }
        let next = expected_difficulty_from_chain(&chain);
        assert!(next < 1000);
    }
}
