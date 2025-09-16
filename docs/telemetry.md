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

Use the `log_context!` macro and `telemetry::log_context()` helper to attach correlation and trace IDs when emitting network, consensus, or storage spans. The node binary exposes `--log-format` (plain or json) and `--log-level` flags for per-module filtering; see `config/logging.json` for an example configuration.

Logs are sampled and rate limited; emitted and dropped counts are exported via `log_emit_total{subsystem}` and `log_drop_total{subsystem}` on the `/metrics` endpoint. A `redact_at_rest` helper can hash or delete log files older than a configured number of hours.
The logger permits up to 100 events per second before sampling kicks in. Once the limit is exceeded, only one out of every 100 events is emitted while the rest are dropped, preventing log bursts from overwhelming block propagation.
Specific subsystems can be disabled at runtime with `telemetry::set_log_enabled`.

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
- `peer_backpressure_active_total{reason}` increments when a peer is throttled for exceeding limits
- `peer_backpressure_dropped_total{reason}` counts requests rejected due to active backpressure
- `p2p_request_limit_hits_total{peer_id}` increments when a peer exceeds its request rate
- `peer_rate_limit_total{peer_id}` records drops due to per-peer rate limiting
- `peer_stats_query_total{peer_id}` counts RPC and CLI lookups
- `peer_stats_reset_total{peer_id}` counts manual metric resets
- `peer_stats_export_total{result}` counts JSON snapshot export attempts (ok, error)
- `peer_stats_export_all_total{result}` counts bulk snapshot exports (ok, error)
- `gateway_dns_lookup_total{status}` counts verified versus rejected DNS entries
- `peer_reputation_score{peer_id}` gauges the dynamic reputation used for rate limits

Additional subsystem counters include:

- `session_key_issued_total`/`session_key_expired_total` track session key lifecycle events
- `wasm_contract_executions_total`/`wasm_gas_consumed_total` report WASM runtime usage
- `difficulty_window_short`/`difficulty_window_med`/`difficulty_window_long` expose EMA windows for the retune algorithm
- `partition_events_total`/`partition_recover_blocks` monitor network partition detection and recovery
- `vm_trace_total` counts debugger trace sessions
- `scheduler_match_total{result}` counts scheduler outcomes (success, capability_mismatch, reputation_failure)
- `scheduler_match_latency_seconds` records time spent matching jobs
- `scheduler_provider_reputation` histogram tracks reputation score distribution
- `scheduler_active_jobs` gauges currently assigned jobs
- `scheduler_preempt_total{reason}` counts job preemption attempts (success or handoff_failed)
- `scheduler_cancel_total{reason}` counts job cancellations (client, provider, preempted)
- `scheduler_effective_price{provider}` gauges the latest effective price per unit by provider
- `price_weight_applied_total` tracks how often reputation weighting adjusted a quoted price
- `scheduler_priority_miss_total` counts high-priority jobs that waited past the
  scheduler's aging threshold

The `scheduler_cancel_total{reason}` counter ties into the compute-market
`compute.job_cancel` RPC, exposing whether cancellations were triggered by the
client, provider, or preemption logic.
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
 - `compute_sla_violations_total{provider}` increments when a provider misses a declared
   SLA, while `compute_provider_uptime{provider}` gauges the rolling uptime percentage.
 - `dex_escrow_locked`, `dex_escrow_pending`, `dex_liquidity_locked_total`,
    `dex_orders_total{side}`, and `dex_trade_volume` expose escrowed funds,
    pending escrows, total locked liquidity, submitted orders, and matched
    quantity across all pairs.
 - `difficulty_retarget_total`, `difficulty_clamp_total` track retarget executions and clamp events.
- `quic_conn_latency_seconds`, `quic_bytes_sent_total`, `quic_bytes_recv_total`, `quic_handshake_fail_total{peer}`, `quic_retransmit_total{peer}`, `quic_cert_rotation_total`, `quic_disconnect_total{code}`, `quic_endpoint_reuse_total` capture QUIC session metrics.
- `release_quorum_fail_total` and `release_installs_total` monitor governance release approvals and installs for dashboards.

### Cluster Aggregator

Deploying the optional `metrics-aggregator` service (see
[`deploy/metrics-aggregator.yaml`](../deploy/metrics-aggregator.yaml)) surfaces
cluster-wide gauges when compiled with `--features telemetry`:

- `cluster_peer_active_total{node_id}` – number of active peers per reporting node (cardinality ≈ node count).
- `aggregator_ingest_total{node_id,result}` – ingestion attempts by node and result (`ok` or `error`), cardinality ≤ node count × 2.
- `log_correlation_fail_total{metric}` – correlation lookups that returned no rows; paired automation triggers `metrics-aggregator` log dumps into `$TB_LOG_DUMP_DIR` when counts spike.

Prometheus query examples:

```promql
# Cancellation rate by reason
sum(rate(scheduler_cancel_total[5m])) by (reason)

# Active peers across the cluster
sum(cluster_peer_active_total)
```

Nodes queue metrics locally if the aggregator is unreachable, so collector
outages do not block operation.

### Telemetry schema and dashboards

The canonical metric list lives in `monitoring/metrics.json`. After adding or
renaming metrics, run `python monitoring/tools/gen_dashboard.py` and commit the
updated `monitoring/grafana/dashboard.json` along with
`monitoring/tests/snapshots/dashboard.json`. This keeps dashboards synchronized
with the schema, and `cargo test -p metrics-aggregator --test naming` enforces
metric naming conventions.

The gauge `banned_peers_total` exposes the number of peers currently banned and
is updated whenever bans are added or expire. Each ban's expiry is also tracked
via `banned_peer_expiration_seconds{peer}`.

Network-level drop behaviour is surfaced via `ttl_drop_total`,
`startup_ttl_drop_total`, and `gossip_ttl_drop_total`, while
`orphan_sweep_total` records the number of
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
`remote_signer_request_total`; any failures bump
`remote_signer_error_total{reason}` so operators can alert on signer
availability.

### Sampling and Compaction

High‑volume counters can be probabilistically sampled to reduce memory
pressure. Configure the global sampling rate with
`telemetry.sample_rate` in `config/default.toml` (1.0 disables
sampling). Sampled counters scale their increments to preserve expected
totals, while histograms simply drop unsampled observations.

Histograms registered for compaction are periodically reset according
to `telemetry.compaction_secs`. Compaction frees internal buckets while
retaining new data. The current telemetry allocation can be inspected
via `cli telemetry dump`, and is exported as the
`telemetry_alloc_bytes` gauge.

Sampling trades precision for lower memory usage; operators requiring
exact counts should keep the sample rate at `1.0`.

