#![cfg(feature = "integration-tests")]
#![cfg(feature = "telemetry")]

use the_block::telemetry;

#[test]
fn log_sampling_rate_limits() {
    use std::time::{SystemTime, UNIX_EPOCH};

    telemetry::reset_log_counters();
    let start = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    while SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        == start
    {
        std::hint::spin_loop();
    }
    telemetry::reset_log_counters();

    for _ in 0..telemetry::LOG_LIMIT {
        assert!(telemetry::should_log("mempool"));
    }

    let extra = telemetry::LOG_SAMPLE_STRIDE * 2;
    let mut logged = 0;
    for _ in 0..extra {
        if telemetry::should_log("mempool") {
            logged += 1;
        }
    }

    assert_eq!(logged, 2, "expected sampling after limit");
    assert_eq!(
        telemetry::LOG_EMIT_TOTAL
            .with_label_values(&["mempool"])
            .get(),
        telemetry::LOG_LIMIT + logged
    );
    assert_eq!(
        telemetry::LOG_DROP_TOTAL
            .with_label_values(&["mempool"])
            .get(),
        extra - logged
    );

    telemetry::reset_log_counters();
}
