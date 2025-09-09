#![cfg(feature = "telemetry")]

use hex;
use the_block::net::{record_request, reset_peer_metrics, set_peer_metrics_sample_rate};
use the_block::telemetry::PEER_REQUEST_TOTAL;

#[test]
fn sampled_request_counter_scales() {
    set_peer_metrics_sample_rate(10);
    let pk = [7u8; 32];
    for _ in 0..1000 {
        record_request(&pk);
    }
    let id = hex::encode(pk);
    let val = PEER_REQUEST_TOTAL.with_label_values(&[id.as_str()]).get();
    assert_eq!(val, 1000);
    reset_peer_metrics(&pk);
    set_peer_metrics_sample_rate(1);
}
