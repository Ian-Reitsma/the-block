#![allow(clippy::result_unit_err)]
#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3::Hasher;
use std::collections::HashSet;

pub type NoteCommitment = [u8; 32];
pub type Nullifier = [u8; 32];

pub mod audit;
pub mod redaction;

/// Simple note structure used for shielded transfers.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub value: u64,
    pub rseed: [u8; 32],
}

impl Note {
    pub fn commitment(&self) -> NoteCommitment {
        let mut h = Hasher::new();
        h.update(&self.value.to_le_bytes());
        h.update(&self.rseed);
        h.finalize().into()
    }

    pub fn nullifier(&self) -> Nullifier {
        let mut h = Hasher::new();
        h.update(&self.commitment());
        h.finalize().into()
    }
}

/// Mempool ensuring nullifier uniqueness.
#[derive(Default)]
pub struct ShieldedMempool {
    seen: HashSet<Nullifier>,
}

impl ShieldedMempool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check_and_insert(&mut self, nf: Nullifier) -> Result<(), ()> {
        if !self.seen.insert(nf) {
            return Err(());
        }
        Ok(())
    }

    pub fn pool_size(&self) -> usize {
        self.seen.len()
    }
}
