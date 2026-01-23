use crate::blockloading::BlockLibrary;
use crate::blocktorch_accelerator::{BlocktorchAccelerator, BlocktorchAcceleratorError, BlocktorchBackend};
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use std::path::{Path, PathBuf};
use std::os::raw::c_int;

/// Metal accelerator implementation that attempts to call into the host runtime.
pub struct MetalAccelerator {
    lib: BlockLibrary,
}

impl MetalAccelerator {
    pub fn new() -> Option<Self> {
        Self::load_lib()
    }

    fn load_lib() -> Option<Self> {
        let path = std::env::var("BLOCKTORCH_METAL_PATH")
            .ok()
            .map(PathBuf::from)
            .or_else(Self::default_path)?;
        BlockLibrary::open(path).map(|lib| Self { lib })
    }

    fn default_path() -> Option<PathBuf> {
        let name = if cfg!(target_os = "macos") {
            "libblocktorch_metal.dylib"
        } else if cfg!(target_os = "linux") {
            "libblocktorch_metal.so"
        } else if cfg!(target_os = "windows") {
            "blocktorch_metal.dll"
        } else {
            return None;
        };
        Some(PathBuf::from(name))
    }

    fn verify_symbol(&self, name: &str) -> Option<VerifyFn> {
        self.lib.symbol(name)
    }

    fn cpu_fallback(
        &self,
        preimage: &[u8],
        verifying_key: &VerifyingKey,
        signature: &Signature,
    ) -> Result<(), BlocktorchAcceleratorError> {
        log::warn!("Metal backend unavailable, delegating to CPU");
        crate::blocktorch_accelerator::CpuAccelerator
            .verify_signature(preimage, verifying_key, signature)
            .map_err(|e| BlocktorchAcceleratorError::Verification {
                reason: format!("metal cpu fallback: {}", e),
            })
    }
}

impl BlocktorchAccelerator for MetalAccelerator {
    fn name(&self) -> &'static str {
        "metal"
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
        let symbol = match self.verify_symbol("blocktorch_metal_verify_signature") {
            Some(sym) => sym,
            None => {
                log::error!("metal verifier symbol missing");
                return self.cpu_fallback(preimage, verifying_key, signature);
            }
        };

        let vk_bytes = verifying_key.to_bytes();
        let sig_bytes = signature.to_bytes();
        let result = unsafe {
            symbol(
                preimage.as_ptr(),
                preimage.len(),
                vk_bytes.as_ptr(),
                sig_bytes.as_ptr(),
            )
        };

        if result == 0 {
            Ok(())
        } else {
            log::warn!("metal verifier returned error code {}", result);
            self.cpu_fallback(preimage, verifying_key, signature)
        }
    }

    fn backend(&self) -> BlocktorchBackend {
        BlocktorchBackend::Metal
    }
}

type VerifyFn = unsafe extern "C" fn(*const u8, usize, *const u8, *const u8) -> c_int;
