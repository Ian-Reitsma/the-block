#![cfg(feature = "integration-tests")]
#![cfg(feature = "telemetry")]

use the_block::{gather_metrics, telemetry};

#[test]
fn metrics_expose_per_subsystem_counters() {
    telemetry::reset_log_counters();
    for sub in ["mempool", "storage", "p2p", "compute"] {
        assert!(telemetry::should_log(sub), "log allowed for {sub}");
    }
    let metrics = gather_metrics().unwrap();
    for sub in ["mempool", "storage", "p2p", "compute"] {
        let emit = format!("log_emit_total{{subsystem=\"{}\"}} 1", sub);
        let drop = format!("log_drop_total{{subsystem=\"{}\"}} 0", sub);
        assert!(metrics.contains(&emit), "missing emit counter for {sub}");
        assert!(metrics.contains(&drop), "missing drop counter for {sub}");
    }
    telemetry::reset_log_counters();
}
