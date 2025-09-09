# Networking Recovery Guide

This guide describes how to restore the distributed hash table (DHT) state when the peer database becomes corrupt or unreachable.

## Resetting the Peer Database
1. **Stop the node** to avoid concurrent writes.
2. Remove the persisted peer list:
   ```bash
   rm ~/.the_block/peers.txt
   ```
   Alternatively point the node at a fresh location by exporting `TB_PEER_DB_PATH`.
3. Optionally pin bootstrap order for tests with:
   ```bash
   export TB_PEER_SEED=1
   ```

## Bootstrapping
1. Start the node and supply at least one known-good peer address:
   ```bash
   cargo run -p the_block --bin node -- run --rpc_addr 127.0.0.1:3030 \
       --data-dir node-data
   ```
   Then edit `~/.the_block/peers.txt` and add `ip:port` entries for trusted peers.  The node will randomize the list on startup.
2. Verify connectivity using the ban utility:
   ```bash
   cargo run -p the_block --bin ban -- --list
   ```
   This prints the current peer set and allows manual removal with `--remove <ip:port>`.
3. Check for handshake failures and DHT convergence via metrics:
   ```bash
   curl -s localhost:9100/metrics | rg '^dht_peers_total'
   ```
   A steadily increasing peer count after bootstrap indicates healthy gossip.

## QUIC Configuration

Nodes may optionally accept gossip over QUIC for reduced handshake latency.
Enable the transport with the `--quic` flag and expose a UDP port. On first
startup a self-signed certificate and private key are generated and written to
`<data_dir>/quic.cert` and `<data_dir>/quic.key`. Subsequent restarts reuse these
files and advertise the QUIC address and certificate during the TCP handshake so
peers can cache and validate the endpoint without manual distribution. Metrics
`quic_conn_latency_seconds`, `quic_bytes_sent_total`, and
`quic_bytes_recv_total` track session performance. Additional counters
`quic_handshake_fail_total`, `net_peer_handshake_fail_total{peer_id,reason}`,
`handshake_fail_total{reason}`, and `quic_disconnect_total{code}` record failed
handshakes and disconnect error codes
for troubleshooting. `quic_endpoint_reuse_total`
counts how often the client connection pool reused an existing endpoint.

Certificates are stored with `0600` permissions and checked for ownership at
startup. The node will regenerate the pair if the files are missing, have
incorrect permissions, or exceed the age specified by
`--quic-cert-ttl-days` (default 30). This allows periodic rotation without
manual intervention.

### QUIC Test Suite

Install `cargo-nextest` and run the networking suite with telemetry enabled:

```bash
cargo nextest run --profile quic --features telemetry
```

Tests use isolated temporary directories and may set `RUST_TEST_THREADS=1`
for reproducibility. Ensure local UDP ports are free before executing the
suite to avoid spurious handshake failures.

### QUIC Handshake Failures and TCP Fallback

If a QUIC handshake fails, the node automatically retries the connection over
TCP. Each failure increments `quic_handshake_fail_total`. A spike in this
counter usually indicates certificate mismatches or blocked UDP traffic. Use
`net stats failures <peer>` to inspect reason-coded counts. When fallback occurs
the gossip message proceeds over the established TCP channel,
so functionality is preserved while operators investigate the root cause.

## Recovery After Corruption
If the peer file was truncated or contained invalid IDs, the discovery layer may misbehave.  After deleting the file and supplying fresh peers as above, restart the node.  The DHT will rebuild automatically and persist the updated peer list on clean shutdown.

These steps can be repeated on any node to recover from corrupted peer databases or during network bootstrapping.

## Peer Database Layout & Configuration

The peer set persists to a flat text file whose location defaults to
`~/.the_block/peers.txt`. The path is resolved by
[`peer_db_path`](../node/src/net/peer.rs) and can be overridden via the
`TB_PEER_DB_PATH` environment variable. Each line holds a single `ip:port`
entry; writes are sorted to keep diffs deterministic. When the node starts
`PeerSet::new` reads this file and merges it with any peers supplied on the
command line.

Chunk gossip uses a separate `SimpleDb` instance. The location defaults to
`~/.the_block/chunks/` and may be changed with `TB_CHUNK_DB_PATH`. Both
directories are created automatically if missing. See
[docs/simple_db.md](simple_db.md) for WAL layout and recovery behavior.

## Rate-Limit Metric Retention

Per-peer telemetry retains up to `max_peer_metrics` entries in memory. The
default cap (1024) prevents unbounded label cardinality in Prometheus. When the
cap is exceeded, the least recently updated peer is evicted and its counters are
removed from the exporter. An informational `evict_peer_metrics` log is emitted
whenever eviction occurs. Each entry tracks requests, bytes, and drops and
consumes only a few dozen bytes, but operators should size the cap according to
expected peer churn and available memory.

Set `peer_metrics_export = false` to suppress per-peer Prometheus labels and
`track_peer_drop_reasons = false` to aggregate all drops under `other` if label
cardinality is a concern.

Rate limits adapt to each peer's reputation score. The base quota is multiplied
by the score, which decays toward `1.0` at a rate of `peer_reputation_decay` and
is reduced by 10% on every rate-limit violation. Inspect scores with
`net stats reputation <peer_id>` and tune decay via `config.toml`.

Per-peer counters are retrievable via the loopback-only `net.peer_stats` RPC or
the `net stats <peer_id>` CLI:

```bash
net stats <peer_id>
```

Each query increments `peer_stats_query_total{peer_id}`. For bulk inspection,
use `net.peer_stats_all` or `net stats --all` with optional `offset` and `limit`
parameters.

Invoking `net.peer_stats_reset` or the `net stats reset` CLI subcommand clears
all counters for the specified peer. This operation removes the peer's metrics
from the Prometheus exporter and permanently discards historical data. Each
successful reset increments `peer_stats_reset_total{peer_id}` which can be
scraped from the metrics endpoint:

```sh
curl -s http://localhost:9898/metrics | grep peer_stats_reset_total
```

Metrics can also be exported for offline inspection using
`net stats export <peer_id> --path <file>`, which writes a JSON snapshot and
increments the `peer_stats_export_total{result}` counter.

### Dynamic Config Reload

Rate-limit quotas and reputation decay can be adjusted without restarting the
node. The watcher monitors `config.toml` and applies new `p2p_max_per_sec` and
`peer_reputation_decay` values when the file changes. Force a reload via:

```bash
net config reload
```

The outcome is logged to telemetry as `config_reload_total{result}`. Sending a
`SIGHUP` to the process triggers the same reload path for systemd integration.
Malformed updates are ignored and the last good configuration is retained.

Persistent snapshots of all peers can be enabled via `peer_metrics_path` in
`config.toml`. On startup, `load_peer_metrics` reads the JSON file, prunes
entries older than `peer_metrics_retention` seconds, and re-registers telemetry
gauges. Operators may manually flush the in-memory map with:

```bash
net stats persist
```

Set `peer_metrics_compress = true` to gzip-compress the on-disk file.

Exports are restricted to `metrics_export_dir` (default `state`) and paths are
validated to prevent traversal. The CLI warns on overwrite and supports
`--all` to create a tarball containing every peer's metrics.

### Peer Throttling

Peers that exceed request or bandwidth quotas are temporarily throttled using
moving averages of `peer_request_total` and `peer_bytes_sent_total`. Quotas are
configured via `p2p_max_per_sec` and `p2p_max_bytes_per_sec` in `config.toml`.
Throttled peers are skipped during Turbine broadcast and accrue reputation
penalties. Inspect a peer's status with:

```bash
net stats <peer_id>
```

The output includes `throttle=<reason>` while a peer is throttled. Operators can
manually engage or clear throttling:

```bash
net stats throttle <peer_id>
net stats throttle <peer_id> --clear
```

Each action increments the `peer_throttle_total{reason}` counter for
observability.

For deterministic tests, setting `TB_PEER_SEED=<u64>` fixes the shuffle order
returned by `PeerSet::bootstrap`. This allows reproducible bootstrap sequences
when running integration tests or chaos simulations.

## RPC Client Timeouts

RPC clients stagger their request retries to avoid thundering herds.  The
following environment variables control timeout behaviour:

- `TB_RPC_TIMEOUT_MS` – base timeout per request (default 5000ms)
- `TB_RPC_TIMEOUT_JITTER_MS` – additional random jitter added to each timeout
  (default 1000ms)
- `TB_RPC_MAX_RETRIES` – number of retry attempts on timeout (default 3)

Set these variables to tune client behaviour in constrained or high latency
networks.

## Fuzzing Peer Identifiers

Malformed peer identifiers should never crash or mis-route. Run the fuzz harness
under `net/fuzz/` to stress the parser:

```bash
RUSTFLAGS="-C instrument-coverage" LLVM_PROFILE_FILE="net/fuzz/peer_id-%p.profraw" \
  cargo +nightly fuzz run peer_id --fuzz-dir net/fuzz -- -runs=100
scripts/fuzz_coverage.sh /tmp/net_cov
```

The coverage script installs missing LLVM tools automatically and merges any
generated `.profraw` files into an HTML report.

## ASN Latency Routing

Peer selection for overlay hops uses an A* search with a latency heuristic. The
implementation and tuning guide live in [`net_a_star.md`](net_a_star.md). In
short, an `AsnLatencyCache` records measured latency floors between ASN pairs
and the heuristic biases routes toward peers with low latency and high uptime.
Operators can adjust `TB_ASTAR_MAX_HOPS` and `TB_ASTAR_CACHE_TTL_MS` to balance
accuracy against CPU overhead. Metrics such as `asn_latency_ms` and
`route_fail_total` surface on the Prometheus exporter for monitoring.

## Gossip Relay Deduplication and Fanout

`node/src/gossip/relay.rs` tracks a TTL map of recently seen messages to drop
duplicates and exposes a `gossip_duplicate_total` counter for monitoring.  When a
message is new, the relay forwards it to `ceil(sqrt(N))` randomly selected peers
(`N` = current peer count, capped at 16) and records the chosen fanout via
`gossip_fanout_gauge`. Setting `TB_GOSSIP_FANOUT=all` forces broadcast to every
peer, a mode intended only for tiny testnets.  See [`docs/gossip.md`](gossip.md)
for a full walkthrough and operational guidance.

## Tie-Break Algorithms and Fork-Injection Fixtures

The gossip layer resolves competing blocks using a deterministic longest-chain rule. Candidates with greater height win; equal-height forks compare cumulative weight and finally the lexicographically smallest tip hash to guarantee convergence. The chaos harness described in [`docs/gossip_chaos.md`](gossip_chaos.md) exercises this logic under 15 % packet loss and 200 ms jitter. Regression tests use the [`node/tests/util/fork.rs`](../node/tests/util/fork.rs) fixture to inject divergent chains and validate that the tie-breaker selects the expected head.
