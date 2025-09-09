# Telemetry Log Fields

Structured telemetry logs include the following fields. All identifiers are privacy-scrubbed with BLAKE3 before emission.

- `subsystem`: originating subsystem (`mempool`, `storage`, `p2p`, or `compute`).
- `op`: short operation code describing the event.
- `sender`: scrubbed sender identifier or account address.
- `nonce`: transaction nonce associated with the event.
- `reason`: human-readable reason for the event.
- `code`: stable numeric code for the event.
- `fpb`: optional fee-per-byte value when applicable.
- `cid`: short correlation identifier derived from a transaction hash or block height.
- `tx`: transaction hash included on all mempool admission and rejection logs for traceability.

Use the `log_context!` macro to attach these correlation IDs when emitting network, consensus, or storage spans. The node binary exposes `--log-format` (plain or json) and `--log-level` flags for per-module filtering; see `config/logging.json` for an example configuration.

Logs are sampled and rate limited; emitted and dropped counts are exported via `log_emit_total{subsystem}` and `log_drop_total{subsystem}` on the `/metrics` endpoint. A `redact_at_rest` helper can hash or delete log files older than a configured number of hours.
The logger permits up to 100 events per second before sampling kicks in. Once the limit is exceeded, only one out of every 100 events is emitted while the rest are dropped, preventing log bursts from overwhelming block propagation.

Counters `peer_error_total{code}` and `rpc_client_error_total{code}` track rate‑limited and banned peers and RPC clients for observability.
All per-peer metrics include a `peer_id` label and, where applicable, a
`reason` label to classify drops or handshake failures. See the
[gossip guide](gossip.md) for RPC and CLI examples.
- `peer_request_total{peer_id}` and `peer_bytes_sent_total{peer_id}` expose per-peer traffic
- `peer_drop_total{peer_id,reason}` classifies discarded messages
- `peer_handshake_fail_total{peer_id,reason}` records QUIC handshake errors
- `peer_metrics_active` gauges the number of peers currently tracked
- `peer_metrics_memory_bytes` approximates memory used by peer metrics
- `peer_throttle_total{reason}` counts peers temporarily throttled for request or bandwidth limits
- `peer_stats_query_total{peer_id}` counts RPC and CLI lookups
- `peer_stats_reset_total{peer_id}` counts manual metric resets
- `peer_stats_export_total{result}` counts JSON snapshot export attempts (ok, error)
- `peer_stats_export_all_total{result}` counts bulk snapshot exports (ok, error)
- `gateway_dns_lookup_total{status}` counts verified versus rejected DNS entries
- `peer_reputation_score{peer_id}` gauges the dynamic reputation used for rate limits
- `scheduler_match_total{result}` counts scheduler outcomes (success, capability_mismatch, reputation_failure)
- `scheduler_match_latency_seconds` records time spent matching jobs
- `scheduler_provider_reputation` histogram tracks reputation score distribution
- `scheduler_active_jobs` gauges currently assigned jobs
- `scheduler_preempt_total{reason}` counts job preemption attempts (success or handoff_failed)
- `scheduler_effective_price{provider}` gauges the latest effective price per unit by provider
- `scheduler_priority_miss_total` counts high-priority jobs that waited past the
  scheduler's aging threshold
- Configuration knobs: `max_peer_metrics` bounds per-peer labels;
  set `peer_metrics_export = false` to disable them,
  `track_peer_drop_reasons = false` to collapse drop reasons,
  and `peer_metrics_sample_rate` to sample high-frequency counters.
  `p2p_max_per_sec` and `p2p_max_bytes_per_sec` define request and bandwidth
  thresholds for throttling.
  Sampling increments `peer_request_total` and `peer_bytes_sent_total`
  every N events and scales by the chosen rate. Larger values reduce
  update overhead but counters may lag by up to `N-1` events. Zero-value
  peer entries are reclaimed periodically to compact memory.
- `industrial_backlog`, `industrial_utilization`, `industrial_units_total`, and
  `industrial_price_per_unit` surface demand for industrial workloads and feed
  `Block::industrial_subsidies()`; see [docs/compute_market.md](compute_market.md)
  for gauge definitions.
- `dex_escrow_locked`, `dex_escrow_pending`, and `dex_escrow_total` expose funds
  locked, the count of outstanding DEX escrows, and the aggregate value of all
  escrowed funds.
- `difficulty_retarget_total`, `difficulty_clamp_total` track retarget executions and clamp events.
- `quic_conn_latency_seconds`, `quic_bytes_sent_total`, `quic_bytes_recv_total`, `quic_handshake_fail_total`, `quic_disconnect_total{code}`, `quic_endpoint_reuse_total` capture QUIC session metrics.

The gauge `banned_peers_total` exposes the number of peers currently banned and
is updated whenever bans are added or expire. Each ban's expiry is also tracked
via `banned_peer_expiration_seconds{peer}`.

Network-level drop behaviour is surfaced via `ttl_drop_total` and
`startup_ttl_drop_total`, while `orphan_sweep_total` records the number of
orphan blocks purged during maintenance passes.

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

Integration tests start the metrics exporter with
`serve_metrics_with_shutdown` so background tasks terminate cleanly. New tests
should do the same to avoid hanging suites.

Histogram `log_size_bytes` records the serialized size of each emitted log.
Panels on the default Grafana dashboard derive average log size from this
histogram, helping operators tune retention and export costs.

## Summary Aggregation and Histograms

Nodes spawn a background task via `telemetry::summary::spawn(interval_secs)` that
periodically calls `emit()` and appends a JSON line to
`telemetry-summary.log`. Each line contains counters and percentile summaries
for selected histograms along with a monotonically increasing sequence number:

```json
{"seq":42,"mempool_size":128,"tx_validation_ms":{"p50":1.2,"p95":3.4}}
```

Available histograms include:

- `tx_validation_ms` – per-transaction validation latency.
- `block_verify_ms` – end-to-end block verification time.
- `match_loop_latency_seconds` – compute-market match cycle.
- `log_size_bytes` – serialized log length.

Buckets follow a base‑2 exponential scheme (`1,2,4,...,65536`) so p50/p99 can be
derived cheaply.  Histograms reset at start-up; summaries report cumulative
statistics since boot.

### CLI Summaries

Operators can inspect the latest snapshot using the CLI:

```bash
blockctl telemetry summarize telemetry-summary.log
```

Sample output:

```text
seq: 42
mempool_size: 128
tx_validation_ms: p50=1.2 p95=3.4
block_verify_ms: p50=45.0 p95=80.0
```

### OTLP Export and Dashboards

Set `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_TIMEOUT` to stream
traces to an external collector.  The default Grafana bundle ships with a
`telemetry-histograms.json` dashboard visualizing the above buckets.
Import it via the Grafana UI or with `make monitor`.

Remote signer requests emit per-call trace IDs and increment
`remote_signer_failure_total` on errors so operators can alert on signer
availability.

