#![forbid(unsafe_code)]

use crypto_suite::hashing::{ripemd160, sha3::Sha3_256};
use crypto_suite::Error as CryptoError;

type Result<T> = core::result::Result<T, CryptoError>;

/// Hash algorithms supported by the HTLC contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgo {
    Sha3,
    Ripemd160,
}

/// Simple Hash Time Locked Contract primitive.
#[derive(Debug, Clone)]
pub struct Htlc {
    pub hash: Vec<u8>,
    pub algo: HashAlgo,
    pub timeout: u64,
    pub redeemed: bool,
}

impl Htlc {
    pub fn new(hash: Vec<u8>, algo: HashAlgo, timeout: u64) -> Self {
        Self {
            hash,
            algo,
            timeout,
            redeemed: false,
        }
    }

    /// Attempts to redeem the HTLC with a preimage at a given time.
    /// Returns true on success.
    pub fn redeem(&mut self, preimage: &[u8], now: u64) -> Result<bool> {
        if self.redeemed || now > self.timeout {
            return Ok(false);
        }
        let computed = match self.algo {
            HashAlgo::Sha3 => {
                let mut h = Sha3_256::new();
                h.update(preimage);
                h.finalize().to_vec()
            }
            HashAlgo::Ripemd160 => ripemd160::hash(preimage)?.to_vec(),
        };
        if computed == self.hash {
            self.redeemed = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Returns true if the contract can be refunded at the given time.
    pub fn refund(&mut self, now: u64) -> bool {
        if now >= self.timeout && !self.redeemed {
            self.redeemed = true;
            true
        } else {
            false
        }
    }
}
