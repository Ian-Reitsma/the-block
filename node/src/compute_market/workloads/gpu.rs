use super::{blocktorch, hash_bytes, BlockTorchWorkloadMetadata, WorkloadRunOutput};
use crypto_suite::hashing::blake3::Hasher;

/// BlockTorch-backed GPU hash workload (Metal when available, deterministic CPU otherwise).
pub fn run(data: &[u8]) -> WorkloadRunOutput {
    let kernel_digest = blocktorch::kernel_bundle_digest();
    let benchmark_commit = blocktorch::runtime_benchmark_commit();
    let descriptor_digest = hash_bytes(b"blocktorch-gpu");
    let output_digest = {
        let (left_bytes, right_bytes) = data.split_at(data.len() / 2);
        let len = left_bytes.len().min(right_bytes.len());
        if len == 0 {
            let mut h = Hasher::new();
            h.update(&kernel_digest);
            h.update(&(data.len() as u64).to_le_bytes());
            *h.finalize().as_bytes()
        } else {
            let left = normalize(left_bytes);
            let right = normalize(right_bytes);
            let mut output = vec![0f32; len];
            let mut h = Hasher::new();
            let used_bridge = blocktorch::add(&left, &right, &mut output);
            h.update(&kernel_digest);
            if let Some(commit) = &benchmark_commit {
                h.update(commit.as_bytes());
            }
            h.update(&(data.len() as u64).to_le_bytes());
            if used_bridge {
                for value in &output {
                    h.update(&value.to_le_bytes());
                }
            } else {
                h.update(data);
            }
            *h.finalize().as_bytes()
        }
    };
    WorkloadRunOutput {
        output: output_digest,
        blocktorch: Some(BlockTorchWorkloadMetadata {
            kernel_digest,
            descriptor_digest,
            output_digest,
            benchmark_commit,
            tensor_profile_epoch: None,
        }),
    }
}

fn normalize(bytes: &[u8]) -> Vec<f32> {
    bytes.iter().map(|b| *b as f32 / 255.0).collect()
}
