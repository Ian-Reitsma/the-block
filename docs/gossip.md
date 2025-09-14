# Gossip Relay Semantics

The gossip layer relays messages between peers while suppressing duplicates and
constraining bandwidth.  `node/src/gossip/relay.rs` implements a TTL-based hash
of serialized messages and stamps outbound traffic with the current partition
marker from `net::partition_watch` so downstream peers can reconcile forks:

```rust
pub fn should_process(&self, msg: &Message) -> bool {
    let h = hash(&bincode::serialize(msg).unwrap_or_default());
    let mut guard = self.recent.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    guard.retain(|_, t| now.duration_since(*t) < self.ttl);
    if guard.contains_key(&h) {
        GOSSIP_DUPLICATE_TOTAL.inc();
        false
    } else {
        guard.insert(h, now);
        true
    }
}
```

The default TTL is two seconds.  Any message seen within that window is dropped
and `gossip_duplicate_total` increments.  Telemetry consumers can monitor this
counter to diagnose misbehaving peers or replay storms.

## Fanout Selection

When a message passes the duplicate check, the relay chooses a random subset of
peers to forward it to.  The fanout size is `ceil(sqrt(N))` capped at 16, where
`N` is the current number of connected peers.  This produces logarithmic spread
without broadcasting to everyone at once.  The chosen fanout is exposed via the
`gossip_fanout_gauge` metric.

For each selected peer the relay consults the peer registry to determine the
preferred transport. If the peer advertises QUIC support the relay uses
`net::quic::send`; otherwise it falls back to the TCP `send_msg` helper. If a
QUIC send fails, the relay retries over TCP to maintain delivery. Session-level
metrics `quic_bytes_sent_total` and `quic_bytes_recv_total` record per-transport
traffic alongside the `gossip_fanout_gauge` gauge.

## Reputation Dissemination

Reputation scores for compute providers propagate through signed
`ReputationUpdate` messages. Each update carries a `provider_id`,
`reputation_score`, and `epoch` timestamp. Nodes accept an update only if the
epoch is newer than their locally persisted snapshot, otherwise
`reputation_gossip_fail_total` increments. Propagation delay is recorded via the
`reputation_gossip_latency_seconds` histogram. Operators can inspect local
scores with `net reputation show <peer>` via the CLI.

Setting the environment variable `TB_GOSSIP_FANOUT=all` disables the random
selection and forces broadcast to every peer.  This override is useful for
small testnets where full fanout is desired.

The selection procedure shuffles the peer list with `rand::thread_rng` and sends
to the first `fanout` entries:

```rust
let mut list = peers.to_vec();
if !fanout_all {
    list.shuffle(&mut rng);
}
for addr in list.into_iter().take(fanout) {
    send(addr, msg);
}
```

Integration tests in `node/tests/gossip_relay.rs` assert that duplicate messages
are dropped and that the computed fanout stays within the expected range even
under packet loss.  The `node/tests/turbine.rs` harness verifies that the
deterministic Turbine tree reaches all peers when the relay fanout equals the
computed `sqrt(N)`.

## Operational Guidance

- Monitor `gossip_duplicate_total` for spikes indicating loops or floods.
- Track `gossip_fanout_gauge` to ensure the relay adapts as peers join or leave.
- Use `TB_GOSSIP_FANOUT=all` only in controlled environments; it negates the
  bandwidth savings of adaptive fanout.
- The default TTL of two seconds balances duplicate suppression with tolerance
  for legitimate replays.  Adjust `Relay::new(ttl)` if your deployment requires
  a different window.

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

Additional flags improve usability:

- `--sort-by latency|drop-rate|reputation` orders peers before display.
- `--filter <regex>` matches peer IDs or addresses.
- `--watch <secs>` refreshes the listing periodically.
- `--summary` prints only aggregate totals.

Latency columns embed a Unicode sparkline scaled to the slowest peer, and table
output clamps to terminal width to avoid wrapping.

```bash
net stats <peer_id>
```

```json
{"method":"net.peer_stats","params":{"peer_id":"abcd…"}}
```

```json
{"method":"net.peer_stats_all","params":{"offset":0,"limit":2}}
```

Example output:

```json
[
  {
    "peer_id": "abcd…",
    "metrics": {"requests": 10, "bytes_sent": 512, "drops": {"rate_limit": 1}}
  },
  {
    "peer_id": "dcba…",
    "metrics": {"requests": 4, "bytes_sent": 128, "drops": 0}
  }
]
```

Configuration knobs:

- `max_peer_metrics` – cap tracked peers to bound memory
- `peer_metrics_export` – disable per‑peer Prometheus labels when `false`
- `track_peer_drop_reasons` – collapse drop reasons into `other` when `false`
- `track_handshake_failures` – disable detailed handshake error labels when `false`
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
