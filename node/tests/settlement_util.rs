#![cfg(feature = "integration-tests")]
use sys::tempfile::{tempdir, TempDir};
use the_block::compute_market::settlement::{self, SettleMode};

pub struct SettlementCtx {
    _dir: TempDir,
}

impl SettlementCtx {
    pub fn new() -> Self {
        Self::with_mode(SettleMode::DryRun)
    }

    pub fn with_mode(mode: SettleMode) -> Self {
        let dir = tempdir().expect("settlement tempdir");
        let path = dir.path().join("settlement");
        let path_str = path.to_str().expect("settlement path must be valid UTF-8");
        settlement::Settlement::init(path_str, mode);
        Self { _dir: dir }
    }
}

impl Drop for SettlementCtx {
    fn drop(&mut self) {
        settlement::Settlement::shutdown();
    }
}
