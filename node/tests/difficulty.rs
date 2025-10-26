#![cfg(feature = "integration-tests")]
use the_block::consensus::difficulty::expected_difficulty_from_chain as expected_difficulty;
use the_block::Block;

fn blank_block(index: u64, ts: u64, diff: u64) -> Block {
    Block {
        index,
        timestamp_millis: ts,
        difficulty: diff,
        base_fee: 1,
        ..Block::default()
    }
}

#[test]
fn retargets_up_when_blocks_fast() {
    let mut chain = Vec::new();
    let mut ts = 1u64;
    for i in 0..120 {
        chain.push(blank_block(i, ts, 1000));
        ts += 500; // half the target spacing
    }
    let next = expected_difficulty(&chain);
    assert_eq!(next, 2000);
}

#[test]
fn retargets_down_when_blocks_slow() {
    let mut chain = Vec::new();
    let mut ts = 1u64;
    for i in 0..120 {
        chain.push(blank_block(i, ts, 1000));
        ts += 2_000; // double the target spacing
    }
    let next = expected_difficulty(&chain);
    assert_eq!(next, 500);
}

#[test]
fn retarget_adjusts() {
    let mut chain = Vec::new();
    let mut ts = 1u64;
    for i in 0..120 {
        chain.push(blank_block(i, ts, 1000));
        ts += 500;
    }
    let up = expected_difficulty(&chain);
    assert_eq!(up, 2000);
    for i in 120..240 {
        chain.push(blank_block(i, ts, up));
        ts += 2_000;
    }
    let down = expected_difficulty(&chain);
    assert_eq!(down, 1000);
}
