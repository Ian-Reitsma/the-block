use std::thread::sleep;
use std::time::Duration;

use the_block::net::{
    clear_peer_metrics, record_request, set_p2p_max_bytes_per_sec, set_p2p_max_per_sec,
};

#[test]
fn throttle_engages_and_recovers() {
    std::env::set_var("TB_THROTTLE_SECS", "1");
    clear_peer_metrics();
    set_p2p_max_per_sec(1);
    set_p2p_max_bytes_per_sec(1000);
    let pk = [1u8; 32];
    for _ in 0..15 {
        record_request(&pk);
    }
    let m = the_block::net::peer_stats(&pk).unwrap();
    assert_eq!(m.throttle_reason.as_deref(), Some("requests"));
    sleep(Duration::from_secs(2));
    record_request(&pk);
    let m2 = the_block::net::peer_stats(&pk).unwrap();
    assert!(m2.throttle_reason.is_none());
}
