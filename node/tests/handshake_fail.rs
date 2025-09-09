#![cfg(feature = "telemetry")]

use the_block::net::{peer_stats, reset_peer_metrics, simulate_handshake_fail, HandshakeError};

#[test]
fn records_handshake_failures() {
    let pk = [9u8; 32];
    simulate_handshake_fail(pk, HandshakeError::Tls);
    simulate_handshake_fail(pk, HandshakeError::Timeout);
    simulate_handshake_fail(pk, HandshakeError::Version);
    simulate_handshake_fail(pk, HandshakeError::Certificate);
    let stats = peer_stats(&pk).expect("stats");
    assert_eq!(stats.handshake_fail.get(&HandshakeError::Tls), Some(&1));
    assert_eq!(stats.handshake_fail.get(&HandshakeError::Timeout), Some(&1));
    assert_eq!(stats.handshake_fail.get(&HandshakeError::Version), Some(&1));
    assert_eq!(stats.handshake_fail.get(&HandshakeError::Certificate), Some(&1));
    assert!(reset_peer_metrics(&pk));
    let stats = peer_stats(&pk).unwrap();
    assert!(stats.handshake_fail.is_empty());
}
