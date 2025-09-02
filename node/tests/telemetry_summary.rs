#[cfg(feature = "telemetry")]
use std::thread::sleep;
#[cfg(feature = "telemetry")]
use std::time::Duration;
#[cfg(feature = "telemetry")]
use the_block::telemetry::summary;

#[cfg(feature = "telemetry")]
#[test]
fn summary_emits() {
    summary::spawn(1);
    sleep(Duration::from_millis(1100));
    assert!(summary::last_count() >= 1);
}
