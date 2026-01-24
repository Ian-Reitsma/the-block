use super::{hash_bytes, BlockTorchWorkloadMetadata};
use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};

/// BlockTorch inference payload description (artifact + input + optional metadata).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BlockTorchInference {
    pub artifact: Vec<u8>,
    pub input: Vec<u8>,
    #[serde(default)]
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub benchmark_commit: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub tensor_profile_epoch: Option<String>,
}

impl BlockTorchInference {
    pub fn new(artifact: Vec<u8>, input: Vec<u8>) -> Self {
        Self {
            artifact,
            input,
            benchmark_commit: None,
            tensor_profile_epoch: None,
        }
    }

    pub fn with_benchmark_commit(mut self, commit: impl Into<String>) -> Self {
        self.benchmark_commit = Some(commit.into());
        self
    }

    pub fn with_tensor_profile_epoch(mut self, epoch: impl Into<String>) -> Self {
        self.tensor_profile_epoch = Some(epoch.into());
        self
    }

    pub fn metadata(&self) -> BlockTorchWorkloadMetadata {
        BlockTorchWorkloadMetadata {
            kernel_digest: hash_bytes(&self.artifact),
            benchmark_commit: self.benchmark_commit.clone(),
            tensor_profile_epoch: self.tensor_profile_epoch.clone(),
        }
    }
}

/// Execute the BlockTorch inference payload via the CPU fallback (hashing the artifact + input).
pub fn run(payload: &BlockTorchInference) -> [u8; 32] {
    let artifact_hash = hash_bytes(&payload.artifact);
    let mut h = Hasher::new();
    h.update(&artifact_hash);
    h.update(&payload.input);
    h.update(&(payload.input.len() as u64).to_le_bytes());
    *h.finalize().as_bytes()
}
