#![cfg(feature = "integration-tests")]
#[cfg(feature = "telemetry")]
use the_block::telemetry;

#[test]
fn histogram_compaction_retains_samples() {
    #[cfg(feature = "telemetry")]
    {
        telemetry::set_sample_rate(1.0);
        telemetry::sampled_observe(&telemetry::QUIC_CONN_LATENCY_SECONDS, 0.5);
        telemetry::force_compact();
        telemetry::sampled_observe(&telemetry::QUIC_CONN_LATENCY_SECONDS, 1.0);
        telemetry::force_compact();
        assert!(telemetry::QUIC_CONN_LATENCY_SECONDS.get_sample_count() > 0);
    }
}
