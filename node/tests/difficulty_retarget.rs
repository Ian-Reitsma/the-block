#![cfg(feature = "integration-tests")]
use testkit::tb_prop_test;
use the_block::consensus::constants::{DIFFICULTY_CLAMP_FACTOR, DIFFICULTY_WINDOW};
use the_block::consensus::difficulty::{
    expected_difficulty_from_chain as expected_difficulty, retarget,
};
use the_block::consensus::difficulty_retune::retune;
use the_block::Block;

#[test]
fn increases_when_blocks_fast() {
    let prev = 1000;
    let params = the_block::governance::Params::default();
    let (next, _) = retune(prev, &[0, 60_000], 0, &params);
    assert!(next > prev);
}

#[test]
fn decreases_when_blocks_slow() {
    let prev = 1000;
    let params = the_block::governance::Params::default();
    let (next, _) = retune(prev, &[0, 240_000], 0, &params);
    assert!(next < prev);
}

fn dummy_block(index: u64, ts: u64, diff: u64) -> Block {
    Block {
        index,
        timestamp_millis: ts,
        difficulty: diff,
        base_fee: 1,
        ..Block::default()
    }
}

tb_prop_test!(retarget_respects_clamp, |runner| {
    runner
        .add_random_case("clamp bounds", 32, |rng| {
            let prev = rng.range_u64(1..=1_000_000);
            let len = rng.range_usize(2..=DIFFICULTY_WINDOW);
            let mut timestamps = Vec::with_capacity(len);
            let mut current = 0u64;
            for _ in 0..len {
                current = current.saturating_add(rng.range_u64(1..=240_000));
                timestamps.push(current);
            }
            let next = retarget(prev, &timestamps, 120_000);
            let min = prev / DIFFICULTY_CLAMP_FACTOR;
            let max = prev.saturating_mul(DIFFICULTY_CLAMP_FACTOR);
            assert!(next >= min && next <= max);
        })
        .expect("register random case");
});

tb_prop_test!(non_monotonic_span_returns_prev, |runner| {
    runner
        .add_random_case("unordered timestamps", 24, |rng| {
            let prev = rng.range_u64(1..=500_000);
            let a = rng.range_u64(0..=1_000_000);
            let b = rng.range_u64(0..=1_000_000);
            let ts = if b <= a { vec![a, b] } else { vec![b, a] };
            let next = retarget(prev, &ts, 120_000);
            assert_eq!(next, prev.max(1));
        })
        .expect("register random case");
});

tb_prop_test!(expected_uses_recent_window, |runner| {
    runner
        .add_random_case("window equivalence", 16, |rng| {
            let prev = rng.range_u64(1..=750_000);
            let prefix_len = rng.range_usize(0..=5);
            let mut prefix = Vec::with_capacity(prefix_len);
            let mut cursor = 0u64;
            for _ in 0..prefix_len {
                cursor = cursor.saturating_add(rng.range_u64(1..=240_000));
                prefix.push(cursor);
            }
            let mut window = Vec::with_capacity(DIFFICULTY_WINDOW);
            let mut tail_cursor = 0u64;
            for _ in 0..DIFFICULTY_WINDOW {
                tail_cursor = tail_cursor.saturating_add(rng.range_u64(1..=240_000));
                window.push(tail_cursor);
            }
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
            assert_eq!(with_prefix, tail_only);
        })
        .expect("register random case");
});
