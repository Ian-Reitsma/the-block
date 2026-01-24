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

/// Metadata extracted from a BlockTorch inference workload.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BlockTorchWorkloadMetadata {
    pub kernel_digest: [u8; 32],
    #[serde(default)]
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub benchmark_commit: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub tensor_profile_epoch: Option<String>,
}
