#![allow(clippy::unwrap_used)]

use std::time::Duration;
use the_block::telemetry;
use the_block::ServiceBadgeTracker;

#[test]
fn badge_issued_after_90_days() {
    let mut tracker = ServiceBadgeTracker::new();
    for _ in 0..90 {
        tracker.record_epoch(true, Duration::from_millis(0));
    }
    assert!(tracker.has_badge());

    // Drop below the revocation threshold.
    for _ in 0..90 {
        tracker.record_epoch(false, Duration::from_millis(0));
    }
    assert!(!tracker.has_badge());
}

#[test]
fn badge_metrics_update() {
    let mut tracker = ServiceBadgeTracker::new();
    for _ in 0..90 {
        tracker.record_epoch(true, Duration::from_millis(0));
    }
    telemetry::BADGE_ACTIVE.set(if tracker.has_badge() { 1 } else { 0 });
    if let Some(ts) = tracker.last_mint() {
        telemetry::BADGE_LAST_CHANGE_SECONDS.set(ts as i64);
    }
    let metrics = telemetry::gather_metrics().unwrap();
    assert!(metrics.contains("badge_active 1"));
    assert!(metrics.contains("badge_last_change_seconds"));
}
