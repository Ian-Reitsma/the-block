#![allow(clippy::unwrap_used)]

use std::time::Duration;
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
