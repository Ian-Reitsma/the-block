# QUIC Transport Integration
> **Review (2025-09-25):** Synced QUIC Transport Integration guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This document outlines the trait-based QUIC transport layer that carries peer-to-peer traffic in The-Block.

## Handshake

Peers establish connections by dialing through `transport::ProviderRegistry`, which selects either the Quinn (default) or s2n-quic backend based on `config/quic.toml`. Each provider advertises its identifier and capability set (`certificate_rotation`, `telemetry_callbacks`, `fingerprint_history`) during the handshake so the `p2p::handshake` module can persist provider metadata, expose it through CLI/RPC surfaces, and route validation through the correct certificate store. Certificates are derived from the node’s Ed25519 network identity, rotated on startup, and cached so peers can migrate without drops. `p2p::handshake` verifies that the peer-presented certificate matches the advertised fingerprint and signer key before upgrading the session.

During startup the registry loads `transport::Config`, instantiates the configured provider, and applies the shared retry/backoff policy. Quinn maintains a pooled set of `Connection` handles to amortise handshakes, while s2n wraps its server/client builders in `Arc` so rotation can happen without recreating listeners. Telemetry callbacks record `quic_conn_latency_seconds`, `quic_handshake_fail_total{peer,provider}`, and the new `quic_provider_connect_total{provider}` counter every time a backend accepts a peer.

## Certificate cache and rotation

Per-peer certificate fingerprints are cached on disk (`~/.the_block/quic_peer_certs.json`) and are now partitioned by provider so history survives backend swaps. The cache is exposed via `transport::CertificateStore::snapshot` and surfaced through `net::peer_cert_snapshot()` for tooling and explorer integration. Retention is governed by `rotation_history` and `rotation_max_age_secs` in `config/quic.toml`.

## Provider registry and configuration

Runtime configuration is split between `config/default.toml` (ports and legacy flags) and `config/quic.toml`, which feeds the transport layer. The following keys map directly onto `transport::Config`:

| Key | Description |
|-----|-------------|
| `provider` | Either `"quinn"` (default) or `"s2n-quic"`; determines which backend the registry instantiates. |
| `certificate_cache` | Optional path used for provider-managed certificates. Reuse the same path when switching providers to preserve history. |
| `retry_attempts` / `retry_backoff_ms` | Listener/connection retry policy passed to the backend. |
| `handshake_timeout_ms` | Timeout before a connect attempt fails (default 5000 ms). |
| `rotation_history` | Number of historical fingerprints retained per provider (default 4). |
| `rotation_max_age_secs` | Maximum age of stored fingerprints in seconds (default 30 days). |

Changing the file and issuing `blockctl config reload` (or `SIGHUP`) triggers the registry to rebuild providers with the new settings; the same path applies on restart.

## Chaos testing

`node/tests/quic_chaos.rs` injects packet loss and duplication to verify session stability across both providers. Metrics `quic_handshake_fail_total{peer,provider}` and `quic_retransmit_total` surface handshake errors and retransmissions. Configure the harness by exporting loss and duplicate ratios (floats or percentages):

```
export TB_QUIC_PACKET_LOSS=0.08
export TB_QUIC_PACKET_DUP=3%
cargo test -p the_block --test quic_chaos --features "integration-tests quic"
```

The test provisions ephemeral Ed25519 certificates, exercises the full QUIC handshake through the configured backend, and asserts that payload delivery plus acknowledgements survive the configured chaos parameters. Loss deltas also appear in the `quic_retransmit_total` counter for telemetry.

For long-running drills, `scripts/chaos.sh` forwards `--quic-loss` and `--quic-dup` flags, populating the same environment variables before bootstrapping a swarm. Summaries can be aggregated with `sim/quic_chaos_summary.rs` to capture retransmit totals and provider mix data for dashboards or post-mortems.

The fuzz corpus at `fuzz/corpus/quic` seeds the `cargo +nightly fuzz run quic_frame` target which repeatedly deserialises QUIC frames to harden bincode parsing. Deterministic seeds keep the harness reproducible under CI.

Certificate rotations are counted via the `quic_cert_rotation_total{provider}` counter, allowing dashboards to correlate cache churn with provider swaps.

## Benchmarks

`node/benches/net_latency.rs` includes a Criterion benchmark that compares QUIC handshakes (through the active transport backend) against a synthetic TCP resend model under the same loss assumptions. Run it with

```
cargo bench -p the_block net_latency --features quic
```

to gauge the relative penalty of retransmits and duplicates when tuning operator settings.

## CLI & Explorer Support

Use `blockctl net quic rotate` to request an on-demand QUIC certificate rotation. The command now returns the new fingerprint, prior fingerprints (hex encoded), the provider identifier, and rotation timestamps for audit tracking. `blockctl net peers --format table` and `blockctl net quic history` surface the same provider-labelled metadata, and RPC consumers receive identical fields for automation.

The explorer publishes the cached peer fingerprints at `GET /network/certs`, returning peer identifiers, provider IDs, and active fingerprints. This view is backed by the on-disk cache, ensuring operators can cross-reference rotations and provider migrations across the fleet.

## Diagnostics

`blockctl net quic-stats --url http://localhost:26658` inspects active sessions and per-peer health. The command shows the last observed handshake latency, retransmission count, endpoint reuse counter, cached jitter, accumulated handshake failures, and the provider ID for each peer. Passing `--json` emits the raw response for scripting and `--token <ADMIN>` forwards an authentication token when the RPC endpoint requires it.

Operators can query the same data via the `net.quic_stats` RPC, which returns the fields surfaced in the CLI alongside provider metadata. Statistics are cached in memory for sub-millisecond responses, telemetry exports `quic_handshake_fail_total{peer,provider}`, `quic_provider_connect_total{provider}`, and retransmit totals, and the metrics-to-logs correlation stack triggers automated log dumps when per-peer failures spike. Large gossip messages continue to prefer QUIC streams when available, falling back to TCP when the transport registry reports a failure.
