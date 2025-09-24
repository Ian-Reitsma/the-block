#![forbid(unsafe_code)]

use p2p_overlay::{OverlayResult, UptimeMetrics};

#[cfg(feature = "telemetry")]
use crate::telemetry::{REBATE_CLAIMS_TOTAL, REBATE_ISSUED_TOTAL};

pub type PeerId = super::OverlayPeerId;

#[cfg(feature = "telemetry")]
pub(super) struct Metrics;

#[cfg(feature = "telemetry")]
impl UptimeMetrics for Metrics {
    fn on_claim(&self) {
        REBATE_CLAIMS_TOTAL.inc();
    }

    fn on_issue(&self) {
        REBATE_ISSUED_TOTAL.inc();
    }
}

pub fn note_seen(peer: PeerId) {
    let service = super::overlay_service();
    service.uptime().note_seen(peer);
}

pub fn eligible(peer: &PeerId, threshold: u64, epoch: u64) -> bool {
    let service = super::overlay_service();
    service.uptime().eligible(peer, threshold, epoch)
}

pub fn claim(peer: PeerId, threshold: u64, epoch: u64, reward: u64) -> Option<u64> {
    let service = super::overlay_service();
    service.uptime().claim(peer, threshold, epoch, reward)
}

pub fn peer_from_bytes(bytes: &[u8]) -> OverlayResult<PeerId> {
    super::overlay_service().peer_from_bytes(bytes)
}
