# Mempool Architecture

The mempool admits, prioritizes, and evicts transactions before they enter a
block.  Implementation details live in [`node/src/lib.rs`](../node/src/lib.rs)
under the `Blockchain` struct, which maintains two lane‑separated DashMaps:
`mempool_consumer` and `mempool_industrial`.

## Admission Pipeline

1. **Precheck** – `canonical_payload_bytes` verifies signatures, nonces, and fee
   selectors before admission.  Basic fee arithmetic ensures `amount + fee`
   does not overflow.
2. **Capacity check** – each lane enforces `max_mempool_size_{consumer,industrial}`
   and rejects excess transactions with `TxAdmissionError::MempoolFull`.
3. **Pending-per-account limit** – `max_pending_per_account` caps the number of
   in-flight nonces per sender to mitigate spam.
4. **Insertion** – the entry is stored as a `MempoolEntry` with the current
   timestamp, monotonic tick, and cached serialized size.

## Priority Queues and Eviction

The mempool orders entries by fee density and expiry.  Eviction uses
`mempool_cmp`, which sorts by:

1. Descending `fee_per_byte` (`fee / serialized_size`)
2. Ascending `expires_at = timestamp + tx_ttl`
3. Lexicographic transaction ID

When the pool is full, the lowest‑priority entry is evicted before admitting a
new one. `tx_ttl` defaults to 300 seconds and is configurable at runtime.

## Spam Detection

The pipeline tracks rejection reasons via telemetry counters such as
`tx_rejected_total{reason="mempool_full"}`.  Pending‑per‑account limits and an
optional external rate limiter protect against sender spam.  The `mempool_mutex`
serialises concurrent access to avoid race conditions.

## Sharded Layout

Each fee lane maintains its own DashMap keyed by `(sender, nonce)`.  Industrial
traffic can therefore be throttled or purged without affecting consumer
transactions.  Future versions may introduce additional shards for geographic or
contract‑specific partitioning.

## Configuration

| Field                          | Purpose                                      | Default |
|--------------------------------|----------------------------------------------|---------|
| `max_mempool_size_consumer`    | Max consumer entries                         | 1024 |
| `max_mempool_size_industrial`  | Max industrial entries                       | 1024 |
| `min_fee_per_byte_consumer`    | Admission floor for consumer lane            | 0 |
| `min_fee_per_byte_industrial`  | Admission floor for industrial lane          | 0 |
| `tx_ttl`                       | Seconds before a tx expires                   | 300 |
| `max_pending_per_account`      | Max in-flight nonces per sender              | 32 |
| `comfort_threshold_p90`        | Fee P90 above which industrial lane defers   | 0 |

Values can be inspected or modified via the Python API or forthcoming RPC
endpoints.

## Snapshot Exports & Debugging

Use `Blockchain::mempool_stats` to retrieve live counts and `Blockchain::pending`
(to be added) to dump sorted entries.  For ad‑hoc inspection, insert `debug!`
logs where `mempool_cmp` is invoked to track eviction decisions.

If the mempool mutex becomes poisoned (e.g., via `poison_mempool`), restart the
node with `heal_mempool()` or wipe the DashMap entries.  Integration tests under
`node/tests/mempool_*` exercise these paths and serve as reference scenarios.
