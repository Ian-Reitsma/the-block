#[cfg(feature = "telemetry")]
#[test]
fn rpc_latency_histogram_records() {
    the_block::telemetry::record_rpc_latency("test", 0.1);
    assert!(the_block::telemetry::rpc_latency_count("test") >= 1);
}
