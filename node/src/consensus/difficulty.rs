use super::constants::{DIFFICULTY_WINDOW, TARGET_SPACING_MS};
use crate::governance::Params;
use crate::Block;

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
