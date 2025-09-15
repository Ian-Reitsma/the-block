# Light Client State Streaming Protocol

Light clients can maintain an up-to-date account view by subscribing to the
state stream exposed over WebSockets. Each `StateChunk` carries:

- `seq` – monotonically increasing sequence number
- `tip_height` – latest observed chain height
- `accounts` – list of `(address, balance)` updates
- `root` – Blake3 Merkle root of the accounts in the chunk
- `proof` – placeholder availability proof bytes
- `compressed` – flag indicating whether this chunk is a zstd-compressed snapshot

Clients apply chunks using `StateStream::apply_chunk`, verifying sequence numbers
and Merkle roots. If packets are dropped, a compressed snapshot may be requested
and applied with `StateStream::apply_snapshot` to resynchronise.

Telemetry metrics include `state_stream_subscribers_total` and
`state_stream_lag_blocks` to alert when clients fall more than the configured
threshold behind. The CLI `light-sync` command bootstraps from the stream by
requesting an initial zstd-compressed snapshot and applying subsequent diffs.
