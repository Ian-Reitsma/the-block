# Macro Block Checkpoints

Macro blocks provide periodic checkpoints summarising shard progress and
reward distribution. A `MacroBlock` is emitted every `macro_interval`
micro‑blocks and contains:

- `height`: micro‑block height at which the checkpoint was taken
- `shard_heights`: latest height observed per shard
- `shard_roots`: state root for each shard at the checkpoint
- `reward_consumer` / `reward_industrial`: total rewards accrued since the
  previous macro block
- `queue_root`: Merkle root of the inter‑shard message queue, allowing
destination shards to verify dequeued messages

Macro blocks are persisted under RocksDB keys of the form
`macro:<height>` and can be inspected via `Blockchain::macro_blocks`.
Explorers surface macro-block height and per-shard roots on `/macro` dashboards
to aid chain audits.
