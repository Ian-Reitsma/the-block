# Light Client State Streaming Protocol
> **Review (2025-09-30):** Documented hybrid snapshot compression and updated telemetry strings.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

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
- `compressed` – flag indicating whether the payload was compressed with the
  in-house `lz77-rle` codec by the sender (currently `false` for live diffs).

## Chunk Processing

`StateStream::apply_chunk` verifies that the incoming sequence number matches the
expected `next_seq`, validates each Merkle proof using the shared
`state::trie::Proof` format, and ensures that every account update carries a
sequence number that is at least as new as the cached value. A failed proof or a
stale account update causes the entire chunk to be rejected. When the client
serializes a chunk (e.g., for guard fixtures or integration snapshots) the
account list is re-ordered lexically by address so `FIRST_PARTY_ONLY` runs
produce identical bytes regardless of the original delivery order.

When a chunk arrives with a higher sequence than expected the stream invokes the
registered gap callback (`StateStream::set_gap_fetcher`). The callback receives
`(from_seq, to_seq)` and should return the missing chunks. The stream applies the
returned chunks—fully verifying proofs—before attempting to apply the original
chunk. If no callback is configured, or if it cannot recover the gap, a
`StateStreamError::GapDetected` or `StateStreamError::GapRecoveryFailed` is
surfaced to the caller.

Clients can tune lag sensitivity with `StateStream::set_lag_threshold`. When
`StateStream::lagging()` detects the stream is more than the configured number of
blocks behind the reported `tip_height`, a warning is emitted via `diagnostics::tracing` so
CLI output and logs highlight the backlog.

## Snapshot Application & Persistence

Full snapshots are applied with `StateStream::apply_snapshot(data, compressed)`.
Snapshots encode a `SnapshotPayload { accounts: Vec<{ address, balance, seq }>,
next_seq }` structure using the first-party
`foundation_serialization::binary` codec with manual serializers on every field.
Account entries are emitted in lexical address order so the snapshot bytes are
stable across runs and `FIRST_PARTY_ONLY` guard rails. A multi-account fixture
(`SNAPSHOT_FIXTURE`) now locks this layout, and unit tests permute account
orders under guard-on/guard-off modes to enforce deterministic bytes. The
client enforces a maximum size derived from the user configuration
(`LightClientConfig::max_snapshot_bytes`) for both compressed and decompressed
data so a malicious server cannot exhaust memory.

Property-driven tests shuffle accounts, vary next-sequence counters, and flip
the compression flag on every iteration to assert the serializer emits the same
bytes regardless of ordering while `StateStream::apply_snapshot` decodes
identically from compressed and uncompressed inputs. Legacy fallback decoding
now receives the same randomized coverage by encoding historical `HashMap`
payloads through `foundation_serialization::binary` and asserting the stream
restores balances with zeroed sequence numbers. These guards keep the
first-party codec honest even as fixtures evolve.

When a snapshot succeeds the in-memory cache is replaced and the next expected
chunk sequence is set to the payload’s `next_seq`. The cache and `next_seq` are
persisted to `~/.the_block/light_state.cache` so mobile clients can resume after
restarts without replaying historical data. Incremental chunk application also
persists the cache after every successful update.

Persisted cache images reuse the same serialization facade and sort the
`accounts` map before encoding, guaranteeing deterministic bytes on disk even
when the in-memory `HashMap` iteration order changes between processes. Guard
parity tests now exercise persisted caches, snapshot fixtures, and chunk
encoders with `FIRST_PARTY_ONLY` forced to `1`, `0`, and unset to prevent future
regressions.

Snapshot processing records telemetry via the `telemetry` feature:

- `the_block_light_state_snapshot_compressed_bytes`
- `the_block_light_state_snapshot_decompressed_bytes`
- `the_block_light_state_snapshot_decompress_errors_total`

These counters capture compression ratios and highlight decompression failures.
Integration tests cover compressed snapshots end-to-end, verifying that
`StateStream::apply_snapshot(.., true)` round-trips bytes emitted by the
in-house `coding::compressor_for("lz77-rle", 4)` facade.

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
user, while lag warnings are logged via `diagnostics::tracing` whenever the stream falls
behind the configured threshold.
