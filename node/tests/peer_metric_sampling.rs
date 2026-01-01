#![cfg(feature = "integration-tests")]
#![cfg(feature = "telemetry")]

use the_block::net::{
    overlay_peer_from_bytes, overlay_peer_to_base58, record_request, reset_peer_metrics,
    set_peer_metrics_sample_rate,
};
use the_block::telemetry;
use the_block::telemetry::PEER_REQUEST_TOTAL;

fn peer_label(pk: &[u8; 32]) -> String {
    overlay_peer_from_bytes(pk)
        .map(|p| overlay_peer_to_base58(&p))
        .unwrap_or_else(|_| crypto_suite::hex::encode(pk))
}

#[test]
fn sampled_request_counter_scales() {
    set_peer_metrics_sample_rate(10);
    let pk = [7u8; 32];
    for _ in 0..1000 {
        record_request(&pk);
    }
    let id = peer_label(&pk);
    let val = PEER_REQUEST_TOTAL
        .ensure_handle_for_label_values(&[id.as_str()])
        .expect(telemetry::LABEL_REGISTRATION_ERR)
        .get();
    assert_eq!(val, 1000);
    reset_peer_metrics(&pk);
    set_peer_metrics_sample_rate(1);
}
