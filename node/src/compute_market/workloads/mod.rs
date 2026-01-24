pub mod blocktorch;
pub mod gpu;
pub mod inference;
pub mod snark;
pub mod transcode;

use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};

pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(data);
    *h.finalize().as_bytes()
}

/// Output of a workload run plus optional BlockTorch metadata for receipts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkloadRunOutput {
    pub output: [u8; 32],
    pub blocktorch: Option<BlockTorchWorkloadMetadata>,
}

impl WorkloadRunOutput {
    pub fn plain(output: [u8; 32]) -> Self {
        Self {
            output,
            blocktorch: None,
        }
    }
}

/// Metadata extracted from a BlockTorch inference workload.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BlockTorchWorkloadMetadata {
    pub kernel_digest: [u8; 32],
    pub descriptor_digest: [u8; 32],
    pub output_digest: [u8; 32],
    #[serde(default)]
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub benchmark_commit: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub tensor_profile_epoch: Option<String>,
}
