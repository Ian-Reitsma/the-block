#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crypto_suite::hashing::blake3;
use testkit::tb_prop_test;
use the_block::compute_market::{Workload, WorkloadRunner};

tb_prop_test!(transcode_hash_matches, |runner| {
    runner
        .add_case("empty payload", || {
            let runner = WorkloadRunner::new();
            let w = Workload::Transcode(Vec::new());
            let out = runtime::block_on(runner.run(0, w));
            let mut h = blake3::Hasher::new();
            h.update(&[]);
            assert_eq!(out, *h.finalize().as_bytes());
        })
        .expect("register deterministic case");

    runner
        .add_random_case("transcode payload", 32, |rng| {
            let data = rng.bytes(0..=256);
            let runner = WorkloadRunner::new();
            let w = Workload::Transcode(data.clone());
            let out = runtime::block_on(runner.run(0, w));
            let mut h = blake3::Hasher::new();
            h.update(&data);
            assert_eq!(out, *h.finalize().as_bytes());
        })
        .expect("register random case");
});

tb_prop_test!(inference_hash_matches, |runner| {
    runner
        .add_case("empty payload", || {
            let runner = WorkloadRunner::new();
            let w = Workload::Inference(Vec::new());
            let out = runtime::block_on(runner.run(0, w));
            let mut h = blake3::Hasher::new();
            h.update(&[]);
            assert_eq!(out, *h.finalize().as_bytes());
        })
        .expect("register deterministic case");

    runner
        .add_random_case("inference payload", 32, |rng| {
            let data = rng.bytes(0..=256);
            let runner = WorkloadRunner::new();
            let w = Workload::Inference(data.clone());
            let out = runtime::block_on(runner.run(0, w));
            let mut h = blake3::Hasher::new();
            h.update(&data);
            assert_eq!(out, *h.finalize().as_bytes());
        })
        .expect("register random case");
});
