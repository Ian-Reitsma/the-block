# Mobile Gateway
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

A lightweight RPC gateway serves mobile clients with a local cache to reduce bandwidth and latency. The cache now provides
durable persistence, bounded growth controls, and telemetry so operators can treat mobile devices as intermittently
connected replicas of the RPC state.

## Cache lifecycle

- Each cached entry carries an insertion timestamp and an expiry deadline. A sweep loop removes entries whose TTL has passed,
  recording both stale and total eviction counters. Internally a min-heap of expirations keeps the oldest entry at the front so
  the sweeper can evict multiple keys per tick without scanning the entire map; tokens guard against removing entries that were
  refreshed after their heap node was enqueued.
- Cache contents, together with the offline transaction queue, are encrypted with ChaCha20-Poly1305 using the node key
  (`TB_MOBILE_CACHE_KEY_HEX` or `TB_NODE_KEY_HEX`). Encrypted blobs are stored in `mobile_cache.db` so restarts retain state.
  Each record stores its own random nonce alongside the ciphertext so multiple services on the same device can safely share a
  key without reuse.
- Operators can tune behaviour with environment variables:
  - `TB_MOBILE_CACHE_TTL_SECS` – entry TTL window (default 300 seconds).
  - `TB_MOBILE_CACHE_SWEEP_SECS` – sweep cadence (default 30 seconds).
  - `TB_MOBILE_CACHE_MAX_ENTRIES` / `TB_MOBILE_CACHE_MAX_BYTES` – per-entry count and payload caps.
  - `TB_MOBILE_CACHE_MAX_QUEUE` – maximum queued offline transactions.
  - `TB_MOBILE_CACHE_DB` – persistence directory for the sled database (default `mobile_cache.db`).

## Offline queue and invalidation

Transactions submitted while offline are queued and replayed on reconnect. Queue depth and buffered bytes are exported as
`mobile_cache_queue_total`, `mobile_tx_queue_depth`, and `mobile_cache_queue_bytes`. Each queued payload stores its enqueue
time so status calls can expose the oldest age, helping operators spot stuck submissions before replay. Upstream writers call
`gateway::mobile_cache::purge_policy` when DNS records mutate to prevent stale responses.

## Telemetry

Telemetry instruments cache hits/misses, eviction totals, rejected insertions, entry/queue gauges, and sweep cadence. The
following Prometheus metrics are exported when the telemetry feature is enabled:

```
mobile_cache_hit_total
mobile_cache_miss_total
mobile_cache_evict_total
mobile_cache_stale_total
mobile_cache_reject_total
mobile_cache_entry_total
mobile_cache_entry_bytes
mobile_cache_queue_total
mobile_cache_queue_bytes
mobile_cache_sweep_total
mobile_cache_sweep_window_seconds
mobile_tx_queue_depth
```

## CLI & RPC controls

- `tb gateway mobile-cache status --url http://localhost:26658 --auth "Bearer <token>"` prints the persisted state. Add
  `--pretty` for formatted JSON.
- `tb gateway mobile-cache flush --url http://localhost:26658 --auth "Bearer <token>"` clears the on-disk cache and offline
  queue.
- The RPC server exposes the same functionality through admin-gated methods `gateway.mobile_cache_status` and
  `gateway.mobile_cache_flush`.

`gateway.mobile_cache_status` returns structured telemetry mirroring the CLI output:

```json
{
  "totals": {
    "hits": 214,
    "misses": 87,
    "stale_evictions": 19,
    "rejections": 3,
    "entry_count": 54,
    "entry_bytes": 28134
  },
  "config": {
    "ttl_secs": 300,
    "sweep_interval_secs": 30,
    "max_entries": 512,
    "max_payload_bytes": 65536,
    "max_queue": 256,
    "db_path": "/var/lib/the-block/mobile_cache.db"
  },
  "entries": [
    { "key": "policy:cdn/assets", "age_secs": 17, "expires_in_secs": 283, "size_bytes": 1280 }
  ],
  "queue": { "depth": 2, "max": 256, "bytes": 842, "oldest_age_secs": 61 }
}
```

Entries are sorted by key; ages rely on monotonic clocks and reset to zero after restarts until the first sweep rebuilds the heap. Watch the `oldest_age_secs` field—values approaching the TTL usually indicate either a stalled upstream or an entry-count ceiling, both of which increment `mobile_cache_reject_total` and merit operator attention.

## Backwards compatibility

Existing deployments should export a cache key derived from the node’s signing key via `TB_MOBILE_CACHE_KEY_HEX` before
upgrading. The gateway automatically migrates prior in-memory data into the encrypted sled store on first run. To migrate
older cache directories:

1. Stop the gateway node and ensure `TB_MOBILE_CACHE_DB` points at the directory that should host `mobile_cache.db`.
2. Back up and remove any legacy plaintext cache files (previous builds sometimes wrote JSON under `gateway/mobile_cache/`).
3. Restart the node; the gateway recreates the sled database and reloads any queued transactions.
4. Run `tb gateway mobile-cache status --auth "Bearer <token>" --pretty` to verify the recovered entries and queue depth.
5. If stale responses remain, run `tb gateway mobile-cache flush --auth "Bearer <token>"` to clear the database and start
   fresh.