#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crypto_suite::hashing::blake3;
use testkit::tb_prop_test;
use the_block::compute_market::{
    workloads,
    workloads::inference::{
        Activation, BlockTorchInference, InputTensorDescriptor, NormalizationPolicy, TensorDtype,
    },
    Workload, WorkloadRunner,
};

fn inference_descriptor(input_len: usize) -> InputTensorDescriptor {
    if input_len == 0 {
        InputTensorDescriptor {
            dtype: TensorDtype::default(),
            input_shape: vec![0],
            weight_shape: (0, 0),
            ..Default::default()
        }
    } else {
        InputTensorDescriptor {
            dtype: TensorDtype::default(),
            input_shape: vec![input_len as u32],
            weight_shape: (1, input_len as u32),
            normalization: NormalizationPolicy::BytesToFloat,
            activation: Activation::Linear,
            ..Default::default()
        }
    }
}

fn build_artifact(seed: &[u8], input_len: usize) -> Vec<u8> {
    let total_bytes = if input_len == 0 { 0 } else { input_len * 4 };
    let mut artifact = Vec::with_capacity(total_bytes);
    while artifact.len() < total_bytes {
        artifact.extend_from_slice(seed);
    }
    artifact.truncate(total_bytes);
    artifact
}

fn inference_payload(data: Vec<u8>) -> BlockTorchInference {
    let descriptor = inference_descriptor(data.len());
    let artifact = build_artifact(&data, data.len());
    BlockTorchInference::new(artifact, data, descriptor)
}

fn inference_workload(input: Vec<u8>) -> Workload {
    Workload::Inference(inference_payload(input))
}

fn expected_inference_output(data: &[u8]) -> [u8; 32] {
    let payload = inference_payload(data.to_vec());
    workloads::inference::run(&payload).output
}

tb_prop_test!(transcode_hash_matches, |runner| {
    runner
        .add_case("empty payload", || {
            let runner = WorkloadRunner::new();
            let w = Workload::Transcode(Vec::new());
            let out = runtime::block_on(runner.run(0, w)).output;
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
            let out = runtime::block_on(runner.run(0, w)).output;
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
            let out = runtime::block_on(runner.run(0, w)).output;
            let expected = expected_inference_output(&[]);
            assert_eq!(out, expected);
        })
        .expect("register deterministic case");

    runner
        .add_random_case("inference payload", 32, |rng| {
            let data = rng.bytes(0..=256);
            let runner = WorkloadRunner::new();
            let w = inference_workload(data.clone());
            let out = runtime::block_on(runner.run(0, w)).output;
            let expected = expected_inference_output(&data);
            assert_eq!(out, expected);
        })
        .expect("register random case");
});
