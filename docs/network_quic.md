# QUIC Transport Integration

This document outlines the deterministic QUIC transport layer used for peer to peer communication.

## Handshake
Peers establish connections using `s2n-quic` with mutual TLS. Certificates are derived from each node's Ed25519 network identity and rotated at startup to limit reuse.

## Chaos Testing
The `tests/quic_chaos.rs` harness injects packet loss and duplication to verify session stability. Metrics `quic_handshake_fail_total` and `quic_retransmit_total` surface handshake errors and retransmissions.

## Diagnostics
Use `net quic-stats` to inspect active sessions and per-peer health. Large gossip messages automatically prefer QUIC streams when available.
