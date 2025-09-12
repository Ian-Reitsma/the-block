# QUIC Transport Handshake

This document describes the QUIC handshake sequence and fallback behaviour within
The-Block networking stack.

## Handshake flow

1. Nodes advertise support for the `QuicTransport` feature bit and include their
   QUIC socket address and certificate in the `p2p::handshake::Hello` message.
2. Peers dial the advertised QUIC endpoint using the [quinn] library.  The
   remote certificate is validated against the provided DER blob and connection
   latency is recorded via `telemetry::QUIC_CONN_LATENCY_SECONDS`.
3. After the transport handshake completes, the standard P2P handshake payload
   is exchanged over the first uni-stream.
4. Handshake failures are classified as version, TLS, certificate, timeout or
   other errors and surfaced via per-peer metrics.

## Transport fallback

If a QUIC connection attempt fails or a peer subsequently reports a TLS error,
`gossip::relay` transparently retries the message over the legacy TCP path. The
peer's transport capability is downgraded to TCP until the next successful
handshake.

## Connection pooling

`net::quic` maintains a small in-memory pool of active `Connection` instances.
Connections are reused across gossip relay and RPC requests, reducing handshake
cost and counted via the `quic_endpoint_reuse_total` metric.

[quinn]: https://github.com/quinn-rs/quinn
