#![forbid(unsafe_code)]

use crypto_suite::hex::encode;
use std::collections::VecDeque;

/// Intent to perform a cross-chain swap via HTLC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtlcIntent {
    pub chain: String,
    pub amount: u64,
    pub hash: Vec<u8>,
    pub timeout: u64,
}

/// Router that matches HTLC swap intents across chains.
#[derive(Default)]
pub struct HtlcRouter {
    pending: VecDeque<HtlcIntent>,
}

impl HtlcRouter {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
        }
    }

    /// Submit a new intent; returns a matched pair if available.
    pub fn submit(&mut self, intent: HtlcIntent) -> Option<(HtlcIntent, HtlcIntent)> {
        if let Some(pos) = self
            .pending
            .iter()
            .position(|i| i.hash == intent.hash && i.amount == intent.amount)
        {
            let other = self.pending.remove(pos).unwrap();
            Some((other, intent))
        } else {
            self.pending.push_back(intent);
            None
        }
    }

    pub fn generate_scripts(a: &HtlcIntent, b: &HtlcIntent) -> (Vec<u8>, Vec<u8>) {
        let sa = format!("htlc:{}:{}", encode(&a.hash), a.timeout);
        let sb = format!("htlc:{}:{}", encode(&b.hash), b.timeout);
        (sa.into_bytes(), sb.into_bytes())
    }
}
