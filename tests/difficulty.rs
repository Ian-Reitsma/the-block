use the_block::blockchain::difficulty::expected_difficulty;
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
        fee_checksum: String::new(),
    }
}

#[test]
fn retargets_up_when_blocks_fast() {
    let mut chain = Vec::new();
    let mut ts = 0u64;
    for i in 0..120 {
        chain.push(blank_block(i, ts, 1000));
        ts += 500; // half the target spacing
    }
    let next = expected_difficulty(&chain);
    assert!(next > 1000);
}

#[test]
fn retargets_down_when_blocks_slow() {
    let mut chain = Vec::new();
    let mut ts = 0u64;
    for i in 0..120 {
        chain.push(blank_block(i, ts, 1000));
        ts += 2_000; // double the target spacing
    }
    let next = expected_difficulty(&chain);
    assert!(next < 1000);
}
