# Gossip Relay Semantics
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The gossip layer relays messages between peers while suppressing duplicates and
constraining bandwidth. `node/src/gossip/relay.rs` now stores recently seen
message digests in a bounded LRU cache so duplicate suppression survives long
uptimes without growing unbounded. Every node persists its relay settings in
`config/gossip.toml`, which controls the TTL, cache capacity, and fanout limits.
On startup the relay loads this configuration, and any edits to
`gossip.toml` (or a `SIGHUP`) reload the settings without restarting the node.

Each message records the current partition marker from
`net::partition_watch`, allowing downstream peers to reconcile forks. The cache
is swept on every lookup; expired entries increment
`gossip_ttl_drop_total` and the duplicate path increments the new
`gossip_peer_failure_total{reason="duplicate"}` counter so operators can spot
looping traffic.

## Fanout Selection

When a message passes the duplicate check, the relay scores each candidate peer
before deciding on a fanout. The score blends the persisted reputation from
`peer_metrics_store`, recent latency hints, and recent failure history.
Low-reputation peers fall below the configured `low_score_cutoff` and are only
contacted when there are not enough high-quality peers to satisfy the minimum
fanout. Peers marked unreachable by `partition_watch` are skipped entirely and
counted in `gossip_peer_failure_total{reason="partition"}`.

The fanout size adapts between the configured `min_fanout`, `base_fanout`, and
`max_fanout` (all set in `gossip.toml`). Healthy clusters with low observed
latency scale toward `max_fanout`, while high failure rates contract toward the
minimum to reduce wasted bandwidth. The chosen fanout and computed latency
observations are exported via `gossip_fanout_gauge` and the
`gossip_latency_seconds` histogram.

For each selected peer the relay consults the peer registry to determine the
preferred transport. If the peer advertises QUIC support the relay uses
`net::quic::send`; otherwise it falls back to the TCP `send_msg` helper. If a
QUIC send fails, the relay retries over TCP to maintain delivery. Session-level
metrics `quic_bytes_sent_total` and `quic_bytes_recv_total` record per-transport
traffic alongside the gossip metrics.

## Reputation Dissemination

Reputation scores for compute providers propagate through signed
`ReputationUpdate` messages. Each update carries a `provider_id`,
`reputation_score`, and `epoch` timestamp. Nodes accept an update only if the
epoch is newer than their locally persisted snapshot, otherwise
`reputation_gossip_fail_total` increments. Propagation delay is recorded via the
`reputation_gossip_latency_seconds` histogram. Operators can inspect local
scores with `net reputation show <peer>` via the CLI.

Setting the environment variable `TB_GOSSIP_FANOUT=all` disables the adaptive
heuristics and forces broadcast to every peer. This override is useful for
small testnets where full fanout is desired.

Property-based regression tests in `node/tests/gossip_relay.rs` assert that the
LRU deduplication window respects the configured TTL and that the adaptive
fanout never exceeds the `gossip.toml` bounds across a wide range of peer
counts.

## Operational Guidance

- Monitor `gossip_duplicate_total` and
  `gossip_peer_failure_total{reason="duplicate"}` for spikes indicating loops
  or floods.
- Track `gossip_latency_seconds` and `gossip_fanout_gauge` to confirm the relay
  adapts as peers join or leave.
- Use `TB_GOSSIP_FANOUT=all` only in controlled environments; it negates the
  bandwidth savings of adaptive fanout.
- Tune `config/gossip.toml` to set the TTL, cache capacity, and fanout bounds
  appropriate for your deployment. Changes take effect immediately thanks to
  the live config watcher.

## Rate-Limit Telemetry

Each peer records request counts, bytes sent, and drop reasons. The following
Prometheus counters expose these metrics:

| Metric | Labels | Description |
|--------|--------|-------------|
| `peer_request_total` | `peer_id` | Total messages received from a peer |
| `peer_bytes_sent_total` | `peer_id` | Bytes delivered to a peer |
| `peer_drop_total` | `peer_id`, `reason` | Messages discarded by reason |

Drop reasons include:

- `rate_limit` – peer exceeded request or shard quotas
- `malformed` – failed basic validation or protocol checks
- `blacklist` – peer banned via `ban` CLI or auto-banning
- `duplicate` – message already seen via Xor8 hash filter

Operators can compare `peer_drop_total{reason="rate_limit"}` with
`peer_request_total` to spot abusive peers. High drop ratios often indicate
misconfigured or malicious nodes and may warrant tighter filters or manual
intervention.

Handshake failures increment `peer_handshake_fail_total{peer_id,reason}` with
reasons like `timeout` or `bad_cert` so operators can diagnose connection
issues.

Each peer also tracks a **reputation score** that scales its allowable request
rate and shard bandwidth. The effective per‑second quota is
`base_limit × reputation`. Scores decay toward `1.0` at a configurable rate and
are multiplied by `0.9` whenever a peer is rate‑limited. Operators can inspect
the current score via `net.peer_stats` or:

```bash
net stats reputation <peer_id>
```

The score is exported as `peer_reputation_score{peer_id}` in Prometheus.

Metrics for an individual peer can be queried over RPC using `net.peer_stats`
or via the `net stats <peer_id>` CLI, returning recent request counts, bytes
sent, and drop totals. Each successful lookup increments the
`peer_stats_query_total{peer_id}` counter and is only available from the
loopback interface. To retrieve metrics for multiple peers at once, use the
`net.peer_stats_all` RPC or `net stats --all`, which accept optional `offset`
and `limit` parameters for pagination. The CLI supports table or JSON output via
`--format table|json`, filtering by drop reason (`--drop-reason`) or minimum
reputation (`--min-reputation`), and interactive paging for large peer sets.
Rows with drop rates ≥5 % show in yellow and ≥20 % in red, and exit codes surface
errors (`0` success, `2` unknown peer, `3` unauthorized):

### Inspecting Runtime State

Operators can inspect the live relay configuration and shard affinities via the
CLI:

```bash
net gossip-status
```

This command calls the new `net.gossip_status` RPC and displays the TTL, cache
occupancy, current fanout bounds, recent adaptive decisions, partition status,
and persisted shard peer lists. Passing `--json` returns the raw JSON payload
for automation.
- `peer_reputation_decay` – rate at which reputation decays toward `1.0`

See [`docs/networking.md`](networking.md) for peer database recovery and
[`docs/gossip_chaos.md`](gossip_chaos.md) for adversarial gossip testing.

### Resetting Metrics

Operators may clear metrics for a specific peer when troubleshooting or after
blocking abusive behavior. Invoke the RPC or CLI reset command:

```json
{"method":"net.peer_stats_reset","params":{"peer_id":"abcd…"}}
```

```bash
net stats reset abcd…
```

Counters for the peer are reset to zero and a `peer_stats_reset_total{peer_id}`
telemetry event is emitted. Historical data is lost and subsequent requests will
recreate counters on demand.

To export metrics for offline analysis, use:

```bash
net stats export <peer_id> --path peer.json
net stats export --all --path bulk [--min-reputation 0.5 --active-within 60]
net stats --all --format json --drop-reason rate_limit --min-reputation 0.8
```

The `net.peer_stats_export` RPC performs the on-disk write. Paths are relative
to `metrics_export_dir` (`state` by default) and sanitized against traversal.
Successful single‑peer exports increment `peer_stats_export_total{result}` and
write a JSON snapshot of the metrics. Supplying `--all` creates a directory with
one `<peer_id>.json` file per peer; enabling `peer_metrics_compress` in the
config compresses the directory to `bulk.tar.gz`. Bulk operations increment
`peer_stats_export_all_total{result}` and log the number of peers and bytes
written. Filters can restrict the export to peers above a reputation threshold
or active within a recent window.

For inspection without writing to disk, the `net.peer_stats_export_all` RPC
returns a JSON object mapping peer IDs to metrics and accepts the same filtering
parameters.

Operator workflows and CLI flag summaries are covered in
[docs/operators/run_a_node.md](operators/run_a_node.md).

## Live Metrics Stream

Nodes expose a WebSocket at `/ws/peer_metrics` that pushes JSON snapshots
whenever a peer's counters change. The stream is restricted to loopback clients
for now and emits messages of the form:

```json
{"peer_id":"abcd…","metrics":{"requests":1,"bytes_sent":0,"drops":{}}}
```

Use the CLI to watch metrics for a specific peer:

```bash
net stats watch abcd…
```

or connect from a browser:

```html
<script>
  const ws = new WebSocket('ws://127.0.0.1:3030/ws/peer_metrics');
  ws.onmessage = (e) => console.log(e.data);
</script>
```

## Peer Key Rotation

Operators may rotate a peer's network key without losing accumulated metrics.
The new key must be signed by the current key to prove authority. Invoke the
CLI or RPC with the stable peer id, new public key, and signature:

```bash
net rotate-key <peer_id> <new_key>
```

Internally this calls `net.key_rotate` which transfers existing metrics to the
new key, emits a `key_rotation_total` telemetry increment, and appends an audit
entry to `state/peer_key_history.log` as well as the metrics aggregator. The old
key is accepted for a brief grace period to allow propagation before revocation.