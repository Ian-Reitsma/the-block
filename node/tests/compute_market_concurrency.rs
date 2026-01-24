#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used)]
use runtime::join_all;
use testkit::tb_prop_test;
use the_block::compute_market::{
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

tb_prop_test!(parallel_runs_deterministic, |runner| {
    runner
        .add_random_case("parallel consistency", 32, |rng| {
            let data = rng.bytes(0..=256);
            let runner = WorkloadRunner::new();
            let w = Workload::Transcode(data);
            let res = runtime::block_on(async {
                let futs: Vec<_> = (0..4).map(|id| runner.run(id, w.clone())).collect();
                join_all(futs).await
            });
            assert!(res.windows(2).all(|win| win[0] == win[1]));
        })
        .expect("register random case");
});

tb_prop_test!(slice_permutation_deterministic, |runner| {
    runner
        .add_random_case("slice permutations", 32, |rng| {
            let data = rng.bytes(0..=128);
            let runner = WorkloadRunner::new();
            let out1 = runtime::block_on(async {
                let futs: Vec<_> = data
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        let w = runner.run(i, Workload::Transcode(vec![*b]));
                        async move { (i, w.await) }
                    })
                    .collect();
                let mut res = join_all(futs).await;
                res.sort_by_key(|(i, _)| *i);
                res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
            });
            let out2 = runtime::block_on(async {
                let futs: Vec<_> = data
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(i, b)| {
                        let w = runner.run(i, Workload::Transcode(vec![*b]));
                        async move { (i, w.await) }
                    })
                    .collect();
                let mut res = join_all(futs).await;
                res.sort_by_key(|(i, _)| *i);
                res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
            });
            assert_eq!(out1, out2);
        })
        .expect("register random case");
});

tb_prop_test!(mixed_workloads_deterministic, |runner| {
    runner
        .add_random_case("mixed workloads", 24, |rng| {
            let data = rng.bytes(0..=128);
            let runner = WorkloadRunner::new();
            let workloads: Vec<_> = data
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    if i % 2 == 0 {
                        Workload::Transcode(vec![*b])
                    } else {
                        inference_workload(vec![*b])
                    }
                })
                .collect();
            let out1 = runtime::block_on(async {
                let futs: Vec<_> = workloads
                    .iter()
                    .enumerate()
                    .map(|(i, w)| {
                        let f = runner.run(i, w.clone());
                        async move { (i, f.await) }
                    })
                    .collect();
                let mut res = join_all(futs).await;
                res.sort_by_key(|(i, _)| *i);
                res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
            });
            let out2 = runtime::block_on(async {
                let futs: Vec<_> = workloads
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(i, w)| {
                        let f = runner.run(i, w.clone());
                        async move { (i, f.await) }
                    })
                    .collect();
                let mut res = join_all(futs).await;
                res.sort_by_key(|(i, _)| *i);
                res.into_iter().map(|(_, h)| h).collect::<Vec<_>>()
            });
            assert_eq!(out1, out2);
        })
        .expect("register random case");
});
