# QUIC Transport Integration

This document outlines the deterministic QUIC transport layer used for peer to peer communication.

## Handshake
Peers establish connections using `s2n-quic` with mutual TLS. Certificates are derived from each node's Ed25519 network identity and rotated at startup to limit reuse.

During startup the node generates a fresh Ed25519-backed X.509 certificate via `net::transport_quic::initialize`. The prior certificate is preserved so peers can migrate gracefully. Gossip messages embed the current certificate fingerprint, and `p2p::handshake` validates that the certificate presented by a peer matches the advertised fingerprint and the peer's signing key.

Per-peer certificate fingerprints are cached on disk (`~/.the_block/quic_peer_certs.json`) so subsequent gossip can reject stale or unknown identities. The cache is exposed through `net::peer_cert_snapshot()` for tooling and the explorer.

## Chaos Testing
The `tests/quic_chaos.rs` harness injects packet loss and duplication to verify session stability. Metrics `quic_handshake_fail_total` and `quic_retransmit_total` surface handshake errors and retransmissions. Configure the harness by exporting loss and duplicate ratios (floats or percentages):

```
export TB_QUIC_PACKET_LOSS=0.08
export TB_QUIC_PACKET_DUP=3%
cargo test -p node --test quic_chaos --features quic
```

The test provisions ephemeral Ed25519 certificates, exercises a full QUIC handshake through the `s2n_quic::provider::io::testing` model, and asserts that payload delivery plus acknowledgements survive the configured chaos parameters. Loss deltas are also reflected in the `quic_retransmit_total` counter for telemetry.

For long running chaos drills, `scripts/chaos.sh` now forwards `--quic-loss` and `--quic-dup` flags which set the same environment variables before bootstrapping a swarm. Run summaries can be aggregated with the helpers in `sim/quic_chaos_summary.rs` to capture success rates and retransmit totals for dashboards or post-mortems.

The fuzz corpus at `fuzz/corpus/quic` seeds the `cargo +nightly fuzz run quic_frame` target which repeatedly deserialises QUIC message frames to harden bincode parsing. Deterministic seeds keep the harness reproducible under CI.

Certificate rotations are counted via the `quic_cert_rotation_total` counter.

## Benchmarks
`node/benches/net_latency.rs` provides a Criterion benchmark that compares QUIC handshakes (through the testing model) against a synthetic TCP resend model under the same loss assumptions. Run it with `cargo bench -p the_block net_latency --features quic` to gauge the relative penalty of retransmits and duplicates when tuning operator settings.

## CLI & Explorer Support

Use `the-block-cli net rotate-cert` to request an on-demand QUIC certificate rotation. The command returns the new fingerprint and prior fingerprints (hex encoded) for audit tracking.

The explorer publishes the current cache of peer fingerprints at `GET /network/certs`, returning peer identifiers and their active fingerprint. This view is backed by the same on-disk cache used by the node, ensuring operators can cross-reference rotations across the cluster.

## Diagnostics
Use `contract-cli net quic-stats --url http://localhost:26658` to inspect active sessions and per-peer health. The command shows the last observed handshake latency, retransmission count, endpoint reuse counter, cached jitter, and accumulated handshake failures for each peer. Passing `--json` emits the raw response for scripting and `--token <ADMIN>` forwards an authentication token when the RPC endpoint requires it.

Operators can query the same data programmatically through the `net.quic_stats` RPC which returns an array of objects containing the fields surfaced in the CLI. Statistics are cached in memory for sub-millisecond responses, telemetry exports `quic_handshake_fail_total{peer="…"}` alongside retransmit totals, and the metrics-to-logs correlation stack triggers automated log dumps when per-peer failures spike. Large gossip messages automatically prefer QUIC streams when available.
