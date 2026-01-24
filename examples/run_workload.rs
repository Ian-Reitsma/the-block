use std::fs;
use the_block::compute_market::{
    workloads::inference::{
        Activation, BlockTorchInference, InputTensorDescriptor, NormalizationPolicy, TensorDtype,
    },
    Workload, WorkloadRunner,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let file = args.next().expect("slice file");
    let kind = args.next().unwrap_or_else(|| "transcode".into());
    let prio = args.next().unwrap_or_else(|| "normal".into());
    let data = fs::read(file).expect("read slice");
    let workload = match kind.as_str() {
        "inference" => {
            let inference = inference_payload(data);
            Workload::Inference(inference)
        }
        "snark" => Workload::Snark(data),
        _ => Workload::Transcode(data),
    };
    let _priority = match prio.as_str() {
        "high" => the_block::compute_market::scheduler::Priority::High,
        "low" => the_block::compute_market::scheduler::Priority::Low,
        _ => the_block::compute_market::scheduler::Priority::Normal,
    };
    let runner = WorkloadRunner::new();
    let out = runtime::block_on(runner.run(0, workload));
    println!("{}", crypto_suite::hex::encode(out.output));
}

fn inference_payload(data: Vec<u8>) -> BlockTorchInference {
    let descriptor = inference_descriptor(data.len());
    let artifact = build_artifact(&data, data.len());
    BlockTorchInference::new(artifact, data, descriptor)
}

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
