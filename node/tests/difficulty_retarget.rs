use proptest::prelude::*;
use the_block::consensus::constants::{DIFFICULTY_CLAMP_FACTOR, DIFFICULTY_WINDOW};
use the_block::consensus::difficulty::{
    expected_difficulty_from_chain as expected_difficulty, retarget,
};
use the_block::{Block, TokenAmount};

#[test]
fn increases_when_blocks_fast() {
    let prev = 1000;
    let next = retarget(prev, &[0, 60_000], 120_000);
    assert!(next > prev);
}

#[test]
fn decreases_when_blocks_slow() {
    let prev = 1000;
    let next = retarget(prev, &[0, 240_000], 120_000);
    assert!(next < prev);
}

fn dummy_block(index: u64, ts: u64, diff: u64) -> Block {
    Block {
        index,
        previous_hash: String::new(),
        timestamp_millis: ts,
        transactions: Vec::new(),
        difficulty: diff,
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
    }
}

proptest! {
    #[test]
    fn retarget_respects_clamp(prev in 1u64..10_000, timestamps in prop::collection::vec(0u64..240_000, 2..=DIFFICULTY_WINDOW)) {
        let next = retarget(prev, &timestamps, 120_000);
        let min = prev / DIFFICULTY_CLAMP_FACTOR;
        let max = prev.saturating_mul(DIFFICULTY_CLAMP_FACTOR);
        prop_assert!(next >= min && next <= max);
    }

    #[test]
    fn non_monotonic_span_returns_prev(prev in 1u64..10_000, a in 0u64..1_000_000, b in 0u64..1_000_000) {
        let ts = if b <= a { vec![a, b] } else { vec![b, a] };
        let next = retarget(prev, &ts, 120_000);
        prop_assert_eq!(next, prev.max(1));
    }

    #[test]
    fn expected_uses_recent_window(
        prev in 1u64..10_000,
        prefix in prop::collection::vec(0u64..1_000_000, 1..50),
        window in prop::collection::vec(0u64..1_000_000, DIFFICULTY_WINDOW)
    ) {
        let mut chain = Vec::new();
        for (i, ts) in prefix.iter().enumerate() {
            chain.push(dummy_block(i as u64, *ts, prev));
        }
        for (i, ts) in window.iter().enumerate() {
            chain.push(dummy_block((prefix.len() + i) as u64, *ts, prev));
        }
        let with_prefix = expected_difficulty(&chain);
        let mut tail_chain = Vec::new();
        for (i, ts) in window.iter().enumerate() {
            tail_chain.push(dummy_block(i as u64, *ts, prev));
        }
        let tail_only = expected_difficulty(&tail_chain);
        prop_assert_eq!(with_prefix, tail_only);
    }
}
