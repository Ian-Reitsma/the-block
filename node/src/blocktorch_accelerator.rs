use concurrency::Lazy;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use std::env;
use std::fmt;
use std::sync::Arc;

mod blocktorch_blockos;
mod blocktorch_cuda;
mod blocktorch_metal;

use blocktorch_blockos::BlockOsAccelerator;
use blocktorch_cuda::CudaAccelerator;
use blocktorch_metal::MetalAccelerator;

/// Trait that exposes a blocktorch acceleration bridge for signature + hash work.
pub trait BlocktorchAccelerator: Send + Sync + 'static {
    /// Human readable name for instrumentation/debugging.
    fn name(&self) -> &'static str;

    /// Returns true if the backend is ready for use on this host.
    fn is_available(&self) -> bool;

    /// Verify an Ed25519 signature.
    fn verify_signature(
        &self,
        preimage: &[u8],
        verifying_key: &VerifyingKey,
        signature: &Signature,
    ) -> Result<(), BlocktorchAcceleratorError>;

    fn backend(&self) -> BlocktorchBackend;
}

/// Errors emitted by the blocktorch accelerator bridge.
#[derive(Debug)]
pub enum BlocktorchAcceleratorError {
    /// Signature verification failed.
    Verification { reason: String },
    /// Requested accelerator is not available.
    Unavailable { reason: String },
}

impl fmt::Display for BlocktorchAcceleratorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlocktorchAcceleratorError::Verification { reason } => {
                write!(f, "verification failed: {}", reason)
            }
            BlocktorchAcceleratorError::Unavailable { reason } => {
                write!(f, "accelerator unavailable: {}", reason)
            }
        }
    }
}

impl std::error::Error for BlocktorchAcceleratorError {}

pub(crate) struct CpuAccelerator;

impl BlocktorchAccelerator for CpuAccelerator {
    fn name(&self) -> &'static str {
        "cpu"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn verify_signature(
        &self,
        preimage: &[u8],
        verifying_key: &VerifyingKey,
        signature: &Signature,
    ) -> Result<(), BlocktorchAcceleratorError> {
        verifying_key.verify(preimage, signature).map_err(|e| {
            BlocktorchAcceleratorError::Verification {
                reason: e.to_string(),
            }
        })
    }

    fn backend(&self) -> BlocktorchBackend {
        BlocktorchBackend::Cpu
    }
}

/// Platforms we can target today or soon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlocktorchBackend {
    Cpu,
    Metal,
    Cuda,
    BlockOs,
}

impl BlocktorchBackend {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "" | "cpu" => Some(BlocktorchBackend::Cpu),
            "metal" => Some(BlocktorchBackend::Metal),
            "cuda" => Some(BlocktorchBackend::Cuda),
            "block_os" | "blockos" => Some(BlocktorchBackend::BlockOs),
            _ => None,
        }
    }

    /// Human readable backend label for telemetry/logging.
    pub fn label(self) -> &'static str {
        match self {
            BlocktorchBackend::Cpu => "cpu",
            BlocktorchBackend::Metal => "metal",
            BlocktorchBackend::Cuda => "cuda",
            BlocktorchBackend::BlockOs => "block_os",
        }
    }
}

impl Default for BlocktorchBackend {
    fn default() -> Self {
        BlocktorchBackend::Cpu
    }
}

/// Create the global accelerator instance.
pub fn global_blocktorch_accelerator() -> Arc<dyn BlocktorchAccelerator> {
    static ACCELERATOR: Lazy<Arc<dyn BlocktorchAccelerator>> = Lazy::new(select_backend);
    Arc::clone(&ACCELERATOR)
}

fn select_backend() -> Arc<dyn BlocktorchAccelerator> {
    match prefer_blocktorch_backend() {
        BlocktorchBackend::Metal => MetalAccelerator::new().unwrap_or_else(fallback),
        BlocktorchBackend::Cuda => CudaAccelerator::new().unwrap_or_else(fallback),
        BlocktorchBackend::BlockOs => BlockOsAccelerator::new().unwrap_or_else(fallback),
        BlocktorchBackend::Cpu => fallback(),
    }
}

fn fallback() -> Arc<dyn BlocktorchAccelerator> {
    Arc::new(CpuAccelerator)
}

fn prefer_blocktorch_backend() -> BlocktorchBackend {
    env::var("BLOCKTORCH_BACKEND")
        .ok()
        .and_then(|v| BlocktorchBackend::from_str(&v))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_suite::signatures::ed25519::{SigningKey, VerifyingKey};
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use rand_core::RngCore;
    use std::env;

    use super::blocktorch_blockos::BlockOsAccelerator;
    use super::blocktorch_cuda::CudaAccelerator;
    use super::blocktorch_metal::MetalAccelerator;

    fn build_preimage() -> Vec<u8> {
        vec![0xde, 0xad, 0xbe, 0xef]
    }

    fn create_test_keypair() -> (SigningKey, VerifyingKey) {
        let mut rng = StdRng::seed_from_u64(0xdeadbeef);
        let mut seed = [0u8; 32];
        rng.fill_bytes(&mut seed);
        let sk = SigningKey::from_bytes(&seed);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    #[test]
    fn cpu_accelerator_verifies_signatures() {
        let (sk, vk) = create_test_keypair();
        let preimage = build_preimage();
        let signature = sk.sign(&preimage);
        let accelerator = CpuAccelerator;
        assert!(accelerator
            .verify_signature(&preimage, &vk, &signature)
            .is_ok());
    }

    #[test]
    fn global_accelerator_is_cpu() {
        let accelerator = global_blocktorch_accelerator();
        assert_eq!(accelerator.name(), "cpu");
        assert!(accelerator.is_available());
    }

    #[test]
    fn backend_parser_accepts_values() {
        assert_eq!(
            BlocktorchBackend::from_str("metal"),
            Some(BlocktorchBackend::Metal)
        );
        assert_eq!(
            BlocktorchBackend::from_str("CUDA"),
            Some(BlocktorchBackend::Cuda)
        );
        assert_eq!(
            BlocktorchBackend::from_str("block_os"),
            Some(BlocktorchBackend::BlockOs)
        );
        assert_eq!(
            BlocktorchBackend::from_str(""),
            Some(BlocktorchBackend::Cpu)
        );
        assert_eq!(BlocktorchBackend::from_str("unknown"), None);
    }

    #[test]
    fn prefer_backend_reads_env() {
        env::set_var("BLOCKTORCH_BACKEND", "cuda");
        assert_eq!(prefer_blocktorch_backend(), BlocktorchBackend::Cuda);
        env::remove_var("BLOCKTORCH_BACKEND");
    }

    #[test]
    fn metal_accelerator_requires_env_flag() {
        env::remove_var("BLOCKTORCH_ALLOW_METAL");
        assert!(MetalAccelerator::new().is_none());
    }

    #[test]
    fn cuda_accelerator_requires_env_flag() {
        env::remove_var("BLOCKTORCH_ALLOW_CUDA");
        assert!(CudaAccelerator::new().is_none());
    }

    #[test]
    fn blockos_accelerator_requires_env_flag() {
        env::remove_var("BLOCKTORCH_ALLOW_BLOCKOS");
        assert!(BlockOsAccelerator::new().is_none());
    }
}
