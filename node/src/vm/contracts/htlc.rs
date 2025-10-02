#![forbid(unsafe_code)]

#[allow(unused_imports)]
use crypto_suite::hashing::sha3::Sha3_256;
#[allow(unused_imports)]
use ripemd::Digest as _;
use ripemd::Ripemd160;

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
    pub fn redeem(&mut self, preimage: &[u8], now: u64) -> bool {
        if self.redeemed || now > self.timeout {
            return false;
        }
        let computed = match self.algo {
            HashAlgo::Sha3 => {
                let mut h = Sha3_256::new();
                h.update(preimage);
                h.finalize().to_vec()
            }
            HashAlgo::Ripemd160 => {
                let mut h = Ripemd160::new();
                h.update(preimage);
                h.finalize().to_vec()
            }
        };
        if computed == self.hash {
            self.redeemed = true;
            true
        } else {
            false
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
