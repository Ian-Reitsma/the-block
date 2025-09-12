use std::time::Duration;
use the_block::service_badge::{current_ts, verify, ServiceBadgeTracker};

#[test]
fn badge_issues_and_revokes() {
    let mut tracker = ServiceBadgeTracker::new();
    for _ in 0..90 {
        tracker.record_epoch(true, Duration::from_millis(1));
    }
    assert!(tracker.has_badge());
    let badge = tracker.current_badge().expect("badge token");
    assert!(verify(&badge));
    for _ in 0..10 {
        tracker.record_epoch(false, Duration::from_millis(1));
    }
    assert!(!tracker.has_badge());
}

#[test]
fn badge_expiration_check() {
    let expired = format!("{:x}", current_ts() - 1);
    assert!(!verify(&expired));
}
