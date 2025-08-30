use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

static LAST_SUMMARY: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

pub fn spawn(interval_secs: u64) {
    if interval_secs == 0 {
        return;
    }
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(interval_secs));
        LAST_SUMMARY.fetch_add(1, Ordering::Relaxed);
    });
}

pub fn last_count() -> u64 {
    LAST_SUMMARY.load(Ordering::Relaxed)
}
