# Telemetry Log Fields

Structured telemetry logs include the following fields. All identifiers are privacy-scrubbed with BLAKE3 before emission.

- `subsystem`: originating subsystem (`mempool`, `storage`, `p2p`, or `compute`).
- `op`: short operation code describing the event.
- `sender`: scrubbed sender identifier or account address.
- `nonce`: transaction nonce associated with the event.
- `reason`: human-readable reason for the event.
- `code`: stable numeric code for the event.
- `fpb`: optional fee-per-byte value when applicable.

Logs are sampled and rate limited; emitted and dropped counts are exported via `log_emit_total{subsystem}` and `log_drop_total{subsystem}` on the `/metrics` endpoint. A `redact_at_rest` helper can hash or delete log files older than a configured number of hours.
The logger permits up to 100 events per second before sampling kicks in. Once the limit is exceeded, only one out of every 100 events is emitted while the rest are dropped, preventing log bursts from overwhelming block propagation.

Counters `peer_error_total{code}` and `rpc_client_error_total{code}` track rate‑limited and banned peers and RPC clients for observability.

The gauge `banned_peers_total` exposes the number of peers currently banned and
is updated whenever bans are added or expire. Each ban's expiry is also tracked
via `banned_peer_expiration_seconds{peer}`.

Manage the persistent ban store with the `ban` CLI:

```bash
ban list               # show active bans and expiration timestamps
ban ban <peer> <secs>  # ban a peer for N seconds
ban unban <peer>       # remove a peer ban
```

Unit tests for the CLI mock the store in memory so no files are written. They
assert that `banned_peers_total` and `banned_peer_expiration_seconds{peer}`
advance on ban/unban and that expired entries are purged on `list`.
When contributing to compute-market or price-board code, run
`cargo nextest run --features telemetry compute_market::courier_retry_updates_metrics price_board`
to verify telemetry and persistence behaviour end-to-end.

Histogram `log_size_bytes` records the serialized size of each emitted log.
Panels on the default Grafana dashboard derive average log size from this
histogram, helping operators tune retention and export costs.
