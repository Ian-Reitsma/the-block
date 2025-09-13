# Real-Time Light Client State Stream

This document outlines the websocket-based protocol used by mobile light
clients to maintain a rolling account cache with minimal bandwidth.

## Protocol Overview

Nodes expose a `/state_stream` websocket endpoint. After a standard
WebSocket handshake, the server pushes JSON-encoded `StateChunk` messages
containing incremental account updates, Merkle roots, and sequence
numbers. Clients apply these diffs to a local cache while verifying the
provided Merkle proofs.

Each chunk carries a `tip_height` field so clients can detect if they are
lagging. When the difference between the advertised tip and the local
sequence exceeds a threshold, a telemetry alert is emitted.

### Snapshots and Compression

New clients may request a compressed snapshot to bootstrap. Snapshots are
zstd-compressed bincode maps of account balances which can be applied via
`StateStream::apply_snapshot`.

## Reliability

Chunks are numbered sequentially. Missing sequence numbers indicate
packet loss and should trigger a snapshot request to re-sync. Tests cover
this flow in `tests/state_stream.rs`.
