use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::VecDeque;
use the_block::consensus::constants::DIFFICULTY_WINDOW;
use the_block::consensus::difficulty::{expected_difficulty, expected_difficulty_from_chain};
use the_block::Block;
use the_block::TokenAmount;

fn sample_chain() -> (VecDeque<u64>, Vec<Block>) {
    let mut recent = VecDeque::new();
    let mut chain = Vec::new();
    let mut ts = 0u64;
    for i in 0..DIFFICULTY_WINDOW {
        recent.push_back(ts);
        chain.push(Block {
            index: i as u64,
            previous_hash: String::new(),
            timestamp_millis: ts,
            transactions: Vec::new(),
            difficulty: 1,
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
        ts += 1_000;
    }
    (recent, chain)
}

fn bench_expected(c: &mut Criterion) {
    let (mut recent, chain) = sample_chain();
    c.bench_function("expected_from_recent", |b| {
        b.iter(|| {
            let slice = recent.make_contiguous();
            expected_difficulty(1, slice)
        })
    });
    c.bench_function("expected_from_chain", |b| {
        b.iter(|| expected_difficulty_from_chain(&chain))
    });
}

criterion_group!(benches, bench_expected);
criterion_main!(benches);
