//! Lightweight DID state tracking for replay protection.
#![forbid(unsafe_code)]

/// Snapshot of the latest anchored DID state for an address.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DidState {
    /// Hash of the anchored DID document.
    pub hash: [u8; 32],
    /// Highest nonce observed for the address.
    pub nonce: u64,
    /// UNIX timestamp (seconds) when the DID was updated.
    pub updated_at: u64,
}

/// Errors encountered when applying DID updates to state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DidStateError {
    /// Incoming update reused or lowered the nonce compared to current state.
    Replay,
}

impl DidState {
    /// Apply an update containing `nonce`, `hash`, and `updated_at` timestamp.
    pub fn apply_update(
        &mut self,
        nonce: u64,
        hash: [u8; 32],
        updated_at: u64,
    ) -> Result<(), DidStateError> {
        if nonce <= self.nonce {
            return Err(DidStateError::Replay);
        }
        self.nonce = nonce;
        self.hash = hash;
        self.updated_at = updated_at;
        Ok(())
    }
}
