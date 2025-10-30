use super::constants::{DIFFICULTY_CLAMP_FACTOR, DIFFICULTY_WINDOW, TARGET_SPACING_MS};
use crate::consensus::difficulty_retune;
use crate::governance::Params;
use crate::Block;

fn scale_timestamps_to_target(timestamps: &[u64], target_spacing_ms: u64) -> Vec<u64> {
    if timestamps.is_empty() {
        return Vec::new();
    }
    let mut scaled = Vec::with_capacity(timestamps.len());
    scaled.push(0);
    let mut prev = timestamps[0];
    let mut accum: u128 = 0;
    let numerator = TARGET_SPACING_MS as u128;
    let denominator = target_spacing_ms.max(1) as u128;
    for &ts in &timestamps[1..] {
        let delta = ts.saturating_sub(prev) as u128;
        let mut scaled_delta = ((delta * numerator) + (denominator / 2)) / denominator;
        if scaled_delta == 0 {
            scaled_delta = 1;
        }
        accum = (accum + scaled_delta).min(u64::MAX as u128);
        scaled.push(accum as u64);
        prev = ts;
    }
    scaled
}

fn scale_difficulty_to_canonical(value: u64, spacing: u64) -> u64 {
    if spacing == TARGET_SPACING_MS {
        return value.max(1);
    }
    let numerator = TARGET_SPACING_MS as u128;
    let denominator = spacing.max(1) as u128;
    let scaled = ((value as u128 * numerator) + (denominator / 2)) / denominator;
    scaled.max(1).min(u64::MAX as u128) as u64
}

fn scale_difficulty_from_canonical(value: u64, spacing: u64) -> u64 {
    if spacing == TARGET_SPACING_MS {
        return value.max(1);
    }
    let numerator = spacing as u128;
    let denominator = TARGET_SPACING_MS as u128;
    let scaled = ((value as u128 * numerator) + (denominator / 2)) / denominator;
    scaled.max(1).min(u64::MAX as u128) as u64
}

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
    let spacing = if target_spacing_ms == 0 {
        TARGET_SPACING_MS
    } else {
        target_spacing_ms
    };
    if spacing == TARGET_SPACING_MS {
        return difficulty_retune::retune(prev, timestamps, 0, &params).0;
    }
    let canonical_prev = scale_difficulty_to_canonical(prev, spacing);
    let scaled_timestamps = scale_timestamps_to_target(timestamps, spacing);
    let (canonical_next, _) =
        difficulty_retune::retune(canonical_prev, &scaled_timestamps, 0, &params);
    let mut next = scale_difficulty_from_canonical(canonical_next, spacing);
    let min = (prev / DIFFICULTY_CLAMP_FACTOR).max(1);
    let max = prev.saturating_mul(DIFFICULTY_CLAMP_FACTOR).max(1);
    if next < min {
        next = min;
    }
    if next > max {
        next = max;
    }
    next
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
    fn retarget_scaled_spacing_matches_canonical() {
        let canonical_prev = 1200;
        let canonical_next = retarget(canonical_prev, &[0, 2_000], TARGET_SPACING_MS);
        let spacing = 120_000;
        let alt_prev = scale_difficulty_from_canonical(canonical_prev, spacing);
        let alt_next = retarget(alt_prev, &[0, 240_000], spacing);
        let expected_alt = scale_difficulty_from_canonical(canonical_next, spacing);
        assert_eq!(alt_next, expected_alt);
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
                read_sub_viewer_ct: TokenAmount::new(0),
                read_sub_host_ct: TokenAmount::new(0),
                read_sub_hardware_ct: TokenAmount::new(0),
                read_sub_verifier_ct: TokenAmount::new(0),
                read_sub_liquidity_ct: TokenAmount::new(0),
                ad_viewer_ct: TokenAmount::new(0),
                ad_host_ct: TokenAmount::new(0),
                ad_hardware_ct: TokenAmount::new(0),
                ad_verifier_ct: TokenAmount::new(0),
                ad_liquidity_ct: TokenAmount::new(0),
                ad_miner_ct: TokenAmount::new(0),
                ad_host_it: TokenAmount::new(0),
                ad_hardware_it: TokenAmount::new(0),
                ad_verifier_it: TokenAmount::new(0),
                ad_liquidity_it: TokenAmount::new(0),
                ad_miner_it: TokenAmount::new(0),
                treasury_events: Vec::new(),
                ad_total_usd_micros: 0,
                ad_settlement_count: 0,
                ad_oracle_ct_price_usd_micros: 0,
                ad_oracle_it_price_usd_micros: 0,
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
