use super::constants::{
    DIFFICULTY_CLAMP_FACTOR, DIFFICULTY_WINDOW, TARGET_SPACING_MS,
};
#[cfg(feature = "telemetry")]
use crate::telemetry::{DIFFICULTY_CLAMP_TOTAL, DIFFICULTY_RETARGET_TOTAL};
use crate::Block;

/// Retarget difficulty using a sliding window of timestamps.
///
/// `prev` is the previous difficulty value.
/// `timestamps` are UNIX **milliseconds** of the most recent blocks,
/// ordered from oldest to newest. The slice length should not
/// exceed `DIFFICULTY_WINDOW`. The `target_millis` represents the
/// desired average block interval in milliseconds.
pub fn retarget(prev: u64, timestamps: &[u64], target_millis: u64) -> u64 {
    if timestamps.len() < 2 {
        return prev.max(1);
    }
    #[cfg(feature = "telemetry")]
    DIFFICULTY_RETARGET_TOTAL.inc();
    // Span between first and last timestamp gives total time for N-1 intervals.
    let span = timestamps.last().unwrap().saturating_sub(timestamps[0]);
    if span == 0 {
        return prev.max(1);
    }
    let blocks = (timestamps.len() - 1) as u64;
    let expected = blocks.saturating_mul(target_millis.max(1));
    let mut next = prev.saturating_mul(expected).saturating_div(span);
    let min = prev / DIFFICULTY_CLAMP_FACTOR;
    let max = prev.saturating_mul(DIFFICULTY_CLAMP_FACTOR);
    if next < min {
        #[cfg(feature = "telemetry")]
        DIFFICULTY_CLAMP_TOTAL.inc();
        next = min;
    }
    if next > max {
        #[cfg(feature = "telemetry")]
        DIFFICULTY_CLAMP_TOTAL.inc();
        next = max;
    }
    next.max(1)
}

/// Compute the expected difficulty for the next block given the previous
/// difficulty and a slice of recent block timestamps.
pub fn expected_difficulty(prev: u64, recent_timestamps: &[u64]) -> u64 {
    retarget(prev.max(1), recent_timestamps, TARGET_SPACING_MS)
}

/// Convenience helper to compute expected difficulty from a full chain slice.
/// This allocates a temporary vector and should be avoided in hot paths.
pub fn expected_difficulty_from_chain(chain: &[Block]) -> u64 {
    if let Some(last) = chain.last() {
        let mut ts: Vec<u64> = chain
            .iter()
            .rev()
            .take(DIFFICULTY_WINDOW)
            .map(|b| b.timestamp_millis)
            .collect();
        ts.reverse();
        expected_difficulty(last.difficulty, &ts)
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
                nonce: 0,
                hash: String::new(),
                coinbase_consumer: TokenAmount::new(0),
                coinbase_industrial: TokenAmount::new(0),
                storage_sub_ct: TokenAmount::new(0),
                read_sub_ct: TokenAmount::new(0),
                compute_sub_ct: TokenAmount::new(0),
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
