use std::time::Duration;
use the_block::service_badge::{ServiceBadgeTracker, verify};

fn main() {
    let mut tracker = ServiceBadgeTracker::new();
    for _ in 0..90 {
        tracker.record_epoch("node", true, Duration::from_millis(1));
    }
    let badge = tracker.current_badge().expect("badge token");
    println!("issued badge: {badge}");
    println!("valid? {}", verify(&badge));
}
