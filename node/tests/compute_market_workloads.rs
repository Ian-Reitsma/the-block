#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crypto_suite::hashing::blake3;
use testkit::tb_prop_test;
use the_block::compute_market::{
    workloads, workloads::inference::BlockTorchInference, Workload, WorkloadRunner,
};

fn inference_workload(input: Vec<u8>) -> Workload {
    let artifact = input.clone();
    let inference = BlockTorchInference::new(artifact, input);
    Workload::Inference(inference)
}

fn expected_inference_output(data: &[u8]) -> [u8; 32] {
    let artifact_hash = workloads::hash_bytes(data);
    let mut h = blake3::Hasher::new();
    h.update(&artifact_hash);
    h.update(data);
    h.update(&(data.len() as u64).to_le_bytes());
    *h.finalize().as_bytes()
}

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
            let w = inference_workload(Vec::new());
            let out = runtime::block_on(runner.run(0, w));
            let expected = expected_inference_output(&[]);
            assert_eq!(out, expected);
        })
        .expect("register deterministic case");

    runner
        .add_random_case("inference payload", 32, |rng| {
            let data = rng.bytes(0..=256);
            let runner = WorkloadRunner::new();
            let w = inference_workload(data.clone());
            let out = runtime::block_on(runner.run(0, w));
            let expected = expected_inference_output(&data);
            assert_eq!(out, expected);
        })
        .expect("register random case");
});
