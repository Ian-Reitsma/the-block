# Light Client State Streaming Protocol

Light clients maintain an up-to-date account view by subscribing to the
state stream exposed over WebSockets. Each `StateChunk` carries:

- `seq` – monotonically increasing chunk sequence number.
- `tip_height` – latest observed chain height from the node.
- `accounts` – list of `AccountChunk { address, balance, account_seq, proof }`.
  - `address` – the account identifier.
  - `balance` – current consumer balance for the account.
  - `account_seq` – per-account monotonic sequence used to reject stale updates.
  - `proof` – Merkle proof (`state::trie::Proof`) attesting to the tuple
    `(balance, account_seq)` under the supplied root.
- `root` – Merkle root for the account entries in the chunk.
- `compressed` – flag indicating whether the payload was zstd-compressed by the
  sender (currently `false` for live diffs).

## Chunk Processing

`StateStream::apply_chunk` verifies that the incoming sequence number matches the
expected `next_seq`, validates each Merkle proof using the shared
`state::trie::Proof` format, and ensures that every account update carries a
sequence number that is at least as new as the cached value. A failed proof or a
stale account update causes the entire chunk to be rejected.

When a chunk arrives with a higher sequence than expected the stream invokes the
registered gap callback (`StateStream::set_gap_fetcher`). The callback receives
`(from_seq, to_seq)` and should return the missing chunks. The stream applies the
returned chunks—fully verifying proofs—before attempting to apply the original
chunk. If no callback is configured, or if it cannot recover the gap, a
`StateStreamError::GapDetected` or `StateStreamError::GapRecoveryFailed` is
surfaced to the caller.

Clients can tune lag sensitivity with `StateStream::set_lag_threshold`. When
`StateStream::lagging()` detects the stream is more than the configured number of
blocks behind the reported `tip_height`, a warning is emitted via `tracing` so
CLI output and logs highlight the backlog.

## Snapshot Application & Persistence

Full snapshots are applied with `StateStream::apply_snapshot(data, compressed)`.
Snapshots encode a `SnapshotPayload { accounts: Vec<{ address, balance, seq }>,
next_seq }` structure via `bincode`. The client enforces a maximum size derived
from the user configuration (`LightClientConfig::max_snapshot_bytes`) for both
compressed and decompressed data so a malicious server cannot exhaust memory.

When a snapshot succeeds the in-memory cache is replaced and the next expected
chunk sequence is set to the payload’s `next_seq`. The cache and `next_seq` are
persisted to `~/.the_block/light_state.cache` so mobile clients can resume after
restarts without replaying historical data. Incremental chunk application also
persists the cache after every successful update.

Snapshot processing records telemetry via the `telemetry` feature:

- `the_block_light_state_snapshot_compressed_bytes`
- `the_block_light_state_snapshot_decompressed_bytes`
- `the_block_light_state_snapshot_decompress_errors_total`

These counters capture compression ratios and highlight decompression failures.

## RPC Expectations

The node WebSocket endpoint (`/rpc/state_stream`) now emits the enriched
`StateChunk` payload, including per-account Merkle proofs and the account
sequence metadata. Servers must construct proofs against the same hashing rule
used by the client (`account_state_value(balance, account_seq)`), ensuring both
sides agree on validation semantics.

## CLI Integration

The `light-sync` CLI command loads `LightClientConfig`, instantiates
`StateStream::from_config`, and applies streamed chunks. Errors such as proof
failures, stale updates, or gap recovery issues are surfaced directly to the
user, while lag warnings are logged via `tracing` whenever the stream falls
behind the configured threshold.
