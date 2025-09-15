#![forbid(unsafe_code)]

use libp2p::PeerId;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "telemetry")]
use crate::telemetry::{REBATE_CLAIMS_TOTAL, REBATE_ISSUED_TOTAL};

#[derive(Default, Clone)]
struct Info {
    last: u64,
    total: u64,
    claimed_epoch: u64,
}

static UPTIME: Lazy<Mutex<HashMap<PeerId, Info>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Note that a peer was seen; updates its uptime counter.
pub fn note_seen(peer: PeerId) {
    let mut map = UPTIME.lock().unwrap();
    let entry = map.entry(peer).or_default();
    let n = now();
    if entry.last > 0 {
        entry.total += n - entry.last;
    }
    entry.last = n;
}

/// Return true if the peer is eligible for a rebate given the threshold in seconds.
pub fn eligible(peer: &PeerId, threshold: u64, epoch: u64) -> bool {
    let map = UPTIME.lock().unwrap();
    if let Some(info) = map.get(peer) {
        info.total >= threshold && info.claimed_epoch < epoch
    } else {
        false
    }
}

/// Claim a rebate for a peer, returning voucher amount if eligible.
pub fn claim(peer: PeerId, threshold: u64, epoch: u64, reward: u64) -> Option<u64> {
    let mut map = UPTIME.lock().unwrap();
    let info = map.entry(peer).or_default();
    if info.total >= threshold && info.claimed_epoch < epoch {
        info.claimed_epoch = epoch;
        #[cfg(feature = "telemetry")]
        {
            REBATE_CLAIMS_TOTAL.inc();
            REBATE_ISSUED_TOTAL.inc();
        }
        Some(reward)
    } else {
        None
    }
}
