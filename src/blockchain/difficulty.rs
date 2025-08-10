use crate::Block;

const TARGET_SPACING_MS: u64 = 1_000;
const WINDOW: usize = 120;

/// Compute the expected difficulty for the next block.
///
/// Uses a moving average over the last `WINDOW` blocks comparing
/// observed spacing to the target cadence. The adjustment is clamped
/// to the range [1/4, 4] of the previous difficulty.
pub fn expected_difficulty(chain: &[Block]) -> u64 {
    if chain.is_empty() {
        return 1;
    }
    let last = chain.len() - 1;
    let current = chain[last].difficulty.max(1);
    let start = if chain.len() > WINDOW {
        chain.len() - WINDOW
    } else {
        0
    };
    let slice = &chain[start..];
    if slice.len() < 2 {
        return current;
    }
    let first_ts = slice.first().unwrap().timestamp_millis;
    let last_ts = slice.last().unwrap().timestamp_millis;
    if first_ts == 0 || last_ts <= first_ts {
        return current;
    }
    let actual = last_ts - first_ts;
    if actual == 0 {
        return current;
    }
    let expected = (slice.len() as u64 - 1) * TARGET_SPACING_MS;
    let ratio = expected as f64 / actual as f64;
    let adj = ratio.clamp(0.25, 4.0);
    let mut next = (current as f64 * adj).round() as u64;
    if next < 1 {
        next = 1;
    }
    next
}
