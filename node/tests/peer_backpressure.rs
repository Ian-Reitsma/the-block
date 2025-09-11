use std::thread::sleep;
use std::time::Duration;

use the_block::net::{
    clear_peer_metrics, peer_stats, record_request, set_p2p_max_bytes_per_sec, set_p2p_max_per_sec,
};

#[test]
fn exponential_backoff_increases_delay() {
    std::env::set_var("TB_THROTTLE_SECS", "1");
    clear_peer_metrics();
    set_p2p_max_per_sec(1);
    set_p2p_max_bytes_per_sec(1000);
    let pk = [2u8; 32];
    for _ in 0..15 {
        record_request(&pk);
    }
    let m1 = peer_stats(&pk).unwrap();
    assert!(m1.throttled_until > 0);
    let first_until = m1.throttled_until;
    sleep(Duration::from_secs(2));
    for _ in 0..15 {
        record_request(&pk);
    }
    let m2 = peer_stats(&pk).unwrap();
    assert!(m2.throttled_until > first_until + 1);
}
