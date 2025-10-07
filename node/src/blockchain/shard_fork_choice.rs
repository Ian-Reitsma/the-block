use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Mutex;

use concurrency::Lazy;
use ledger::address::ShardId;

#[derive(Clone, Copy)]
struct Candidate {
    hash: [u8; 32],
    difficulty: u64,
    macro_height: u64,
}

static TIPS: Lazy<Mutex<HashMap<ShardId, Vec<Candidate>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Register a candidate tip for a shard.
pub fn submit_tip(shard: ShardId, hash: [u8; 32], difficulty: u64, macro_height: u64) {
    let mut map = TIPS.lock().unwrap();
    map.entry(shard).or_default().push(Candidate {
        hash,
        difficulty,
        macro_height,
    });
}

/// Select the best PoW tip for a shard.
///
/// Highest difficulty wins, with macro-block height as a tie-breaker to
/// enforce cross-shard ordering.
pub fn select_tip(shard: ShardId) -> Option<[u8; 32]> {
    let map = TIPS.lock().unwrap();
    map.get(&shard).and_then(|candidates| {
        candidates
            .iter()
            .max_by(|a, b| match a.difficulty.cmp(&b.difficulty) {
                Ordering::Equal => a.macro_height.cmp(&b.macro_height),
                other => other,
            })
            .map(|c| c.hash)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        TIPS.lock().unwrap().clear();
    }

    #[test]
    fn picks_highest_difficulty() {
        reset();
        submit_tip(1, [1u8; 32], 5, 10);
        submit_tip(1, [2u8; 32], 6, 9);
        assert_eq!(select_tip(1), Some([2u8; 32]));
    }

    #[test]
    fn macro_height_tiebreaker() {
        reset();
        submit_tip(2, [3u8; 32], 7, 8);
        submit_tip(2, [4u8; 32], 7, 9);
        assert_eq!(select_tip(2), Some([4u8; 32]));
    }
}
