use crate::blockloading::BlockLibrary;
use crate::blocktorch_accelerator::{BlocktorchAccelerator, BlocktorchAcceleratorError, BlocktorchBackend};
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use std::path::PathBuf;
use std::os::raw::c_int;

/// BlockOS accelerator loader bridging to libc.
pub struct BlockOsAccelerator {
    lib: BlockLibrary,
}

impl BlockOsAccelerator {
    pub fn new() -> Option<Self> {
        let path = std::env::var("BLOCKTORCH_BLOCKOS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("libblocktorch_blockos.so"));
        BlockLibrary::open(path).map(|lib| Self { lib })
    }

    fn symbol(&self) -> Option<VerifyFn> {
        self.lib.symbol("blocktorch_blockos_verify_signature")
    }

    fn cpu_fallback(
        &self,
        preimage: &[u8],
        verifying_key: &VerifyingKey,
        signature: &Signature,
    ) -> Result<(), BlocktorchAcceleratorError> {
        log::warn!("BlockOS backend unavailable, falling back to CPU");
        crate::blocktorch_accelerator::CpuAccelerator
            .verify_signature(preimage, verifying_key, signature)
            .map_err(|e| BlocktorchAcceleratorError::Verification {
                reason: format!("block_os cpu fallback: {}", e),
            })
    }
}

impl BlocktorchAccelerator for BlockOsAccelerator {
    fn name(&self) -> &'static str {
        "block_os"
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
        let symbol = match self.symbol() {
            Some(sym) => sym,
            None => {
                log::error!("block_os verifier symbol missing");
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
            log::warn!("block_os verifier returned error code {}", result);
            self.cpu_fallback(preimage, verifying_key, signature)
        }
    }

    fn backend(&self) -> BlocktorchBackend {
        BlocktorchBackend::BlockOs
    }
}

type VerifyFn = unsafe extern "C" fn(*const u8, usize, *const u8, *const u8) -> c_int;
