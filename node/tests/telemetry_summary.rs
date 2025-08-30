use std::thread::sleep;
use std::time::Duration;
use the_block::telemetry::summary;

#[test]
fn summary_emits() {
    summary::spawn(1);
    sleep(Duration::from_millis(1100));
    assert!(summary::last_count() >= 1);
}
