#![cfg(all(feature = "integration-tests", feature = "telemetry"))]
use the_block::{
    gateway,
    rpc::analytics::{self, AnalyticsQuery},
    telemetry::READ_STATS,
};

#[test]
fn read_stats_reflect_append() {
    let tmp = sys::tempfile::tempdir().unwrap();
    std::env::set_var("TB_GATEWAY_RECEIPTS", tmp.path());
    gateway::read_receipt::append("example.com", "gateway", 123, false, true).unwrap();
    let q = AnalyticsQuery {
        domain: "example.com".into(),
    };
    let stats = analytics::analytics(&READ_STATS, q);
    assert_eq!(stats.reads, 1);
    assert_eq!(stats.bytes, 123);
}
