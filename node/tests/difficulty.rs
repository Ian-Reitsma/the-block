use the_block::consensus::difficulty::expected_difficulty_from_chain as expected_difficulty;
use the_block::{Block, TokenAmount};

fn blank_block(index: u64, ts: u64, diff: u64) -> Block {
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
