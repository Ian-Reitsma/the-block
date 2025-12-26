#![cfg(feature = "integration-tests")]
#[cfg(feature = "telemetry")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "telemetry")]
use std::thread;
#[cfg(feature = "telemetry")]
use std::time::Duration;

#[cfg(feature = "telemetry")]
use the_block::range_boost::{self as range_boost, FaultMode};
use the_block::range_boost::{HopProof, RangeBoost};

#[cfg(feature = "telemetry")]
use the_block::telemetry::{
    RANGE_BOOST_ENQUEUE_ERROR_TOTAL, RANGE_BOOST_FORWARDER_FAIL_TOTAL,
    RANGE_BOOST_TOGGLE_LATENCY_SECONDS,
};

#[test]
fn bundle_queue_works() {
    let mut rb = RangeBoost::new();
    rb.enqueue(vec![0u8; 4]);
    rb.record_proof(
        0,
        HopProof {
            relay: "loopback".into(),
        },
    );
    let entry = rb.dequeue().unwrap();
    assert_eq!(entry.payload.len(), 4);
    assert_eq!(entry.proofs[0].relay, "loopback");
}

#[test]
fn parse_packet() {
    let data = b"unix:/tmp/sock,42";
    let peer = the_block::range_boost::parse_discovery_packet(data).unwrap();
    assert_eq!(peer.addr, "unix:/tmp/sock");
    assert_eq!(peer.latency_ms, 42);
}

#[test]
fn range_boost_toggle_stress() {
    for i in 0..256 {
        let enable = i % 2 == 0;
        the_block::range_boost::set_enabled(enable);
        assert_eq!(the_block::range_boost::is_enabled(), enable);
    }
    the_block::range_boost::set_enabled(false);
    assert!(!the_block::range_boost::is_enabled());
}

#[cfg(feature = "telemetry")]
#[test]
fn range_boost_toggle_latency_records_histogram() {
    let start = RANGE_BOOST_TOGGLE_LATENCY_SECONDS.get_sample_count();
    for i in 0..32 {
        let enable = i % 2 == 0;
        range_boost::set_enabled(enable);
    }
    range_boost::set_enabled(false);
    assert!(RANGE_BOOST_TOGGLE_LATENCY_SECONDS.get_sample_count() > start);
}

#[cfg(feature = "telemetry")]
#[test]
fn range_boost_fault_injection_counts_failures() {
    let baseline = RANGE_BOOST_FORWARDER_FAIL_TOTAL.value();
    let queue = Arc::new(Mutex::new(RangeBoost::new()));
    range_boost::spawn_forwarder(&queue);
    range_boost::set_enabled(true);
    range_boost::set_forwarder_fault_mode(FaultMode::ForceEncode);
    queue.lock().unwrap().enqueue(vec![1, 2, 3, 4]);
    for _ in 0..20 {
        if RANGE_BOOST_FORWARDER_FAIL_TOTAL.value() > baseline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    range_boost::set_forwarder_fault_mode(FaultMode::None);
    range_boost::set_enabled(false);
    assert!(RANGE_BOOST_FORWARDER_FAIL_TOTAL.value() > baseline);
}

#[cfg(feature = "telemetry")]
#[test]
fn range_boost_enqueue_injection_drops_bundle() {
    let baseline = RANGE_BOOST_ENQUEUE_ERROR_TOTAL.value();
    range_boost::inject_enqueue_error();
    let mut rb = RangeBoost::new();
    rb.enqueue(vec![9]);
    assert_eq!(rb.pending(), 0);
    assert_eq!(RANGE_BOOST_ENQUEUE_ERROR_TOTAL.value(), baseline + 1);
}
