# QUIC Transport Handshake
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

This document describes the QUIC handshake sequence, provider metadata, and fallback behaviour within The-Block networking stack.

## Handshake flow

1. Nodes advertise the `QuicTransport` feature bit, their QUIC socket address, certificate fingerprint, provider identifier, and capability bitmap in the `p2p::handshake::Hello` message.
2. Peers dial the advertised QUIC endpoint through `transport::ProviderRegistry`, which instantiates the configured backend (Quinn by default, s2n-quic when enabled). The registry supplies a shared `transport::Config` derived from `config/quic.toml`, covering retry/backoff policy, handshake timeout, and certificate cache paths.
3. The remote certificate is validated via the provider’s `CertificateStore`, which mirrors fingerprints from disk and enforces rotation history. Handshake latency, provider name, and failure reasons are emitted through the transport callbacks (`quic_conn_latency_seconds`, `quic_handshake_fail_total{peer,provider}`, `quic_provider_connect_total{provider}`).
4. After the transport handshake completes, the standard P2P payload proceeds over the first uni-stream. The handshake layer persists the provider metadata so CLI and RPC surfaces can display which implementation each peer is using.

## Transport fallback

If a QUIC connection attempt fails or a peer subsequently reports a TLS error, `gossip::relay` transparently retries the message over the legacy TCP path. The peer’s transport capability is downgraded to TCP until the next successful handshake, and provider failure counters continue to increment for telemetry. Operators can filter the CLI/RPC outputs by provider to spot unhealthy backends.

## Connection pooling

`crates/transport::quinn_backend` maintains a small pool of active `Connection` handles that cover gossip relay and RPC requests, reducing handshake overhead. The pool tracks the provider identifier in metrics (`quic_endpoint_reuse_total{provider}`) and preserves telemetry callbacks even when the provider is hot-swapped during a config reload.

## Provider registry and configuration

`transport::Config` is loaded from `config/quic.toml` at startup and on every reload. Quinn remains the default provider and exposes integration-friendly insecure-connect helpers. Setting `provider = "s2n-quic"` switches to the s2n backend while sharing the same telemetry hooks for handshake failures, rotations, and retransmits. Additional keys control retry behaviour (`retry_attempts`, `retry_backoff_ms`), the handshake timeout, and the certificate cache path used when persisting rotation state. Reloading the file or sending `SIGHUP` prompts the registry to rebuild providers so the change takes effect without a restart.

## Pivot alignment

The transport abstraction is phase five of the dependency-sovereignty roadmap
documented in [`docs/pivot_dependency_strategy.md`](pivot_dependency_strategy.md).
Governance parameters will eventually gate the selected provider; until then,
operators should track `quic_provider_connect_total{provider}` and related
metrics to evaluate rollout readiness before voting on backend switches.

[quinn]: https://github.com/quinn-rs/quinn