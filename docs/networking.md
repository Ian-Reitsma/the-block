# Networking Recovery Guide
> **Review (2025-09-25):** Synced Networking Recovery Guide guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This guide describes how to restore the distributed hash table (DHT) state when the peer database becomes corrupt or unreachable.
Overlay abstraction progress and dependency controls are tracked in
[`docs/pivot_dependency_strategy.md`](pivot_dependency_strategy.md); consult that
plan when swapping libp2p backends or testing the upcoming `crates/p2p_overlay`
crate.

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

## Overlay Backend Troubleshooting

Overlay swaps now rely on the `crates/p2p_overlay` abstraction. When a node
starts, the configuration loader performs a sanity check to confirm the active
overlay backend matches `config/default.toml`; failures print
`overlay_sanity_failed` along with remediation steps. Use the following flow
when diagnostics disagree with the configured backend:

1. Inspect the live overlay snapshot:
   ```bash
   the-block net overlay-status --format json
   # Legacy --json remains accepted for backward compatibility
   ```
   (The legacy `--json` flag is still accepted.) The output includes the active
   backend label, tracked peer count, and the persisted database path (if
   applicable). When the CLI reports `stub` or an unexpected path, restart the
   node after updating configuration.
2. Confirm Prometheus gauges `overlay_backend_active{backend}`,
   `overlay_peer_total{backend}`, and `overlay_peer_persisted_total{backend}` are
   reporting values for the intended backend:
   ```bash
   curl -s localhost:9100/metrics | rg '^overlay_'
   ```
   Only the selected backend should emit non-zero values.
3. When a mismatch persists, delete the stale peer database (libp2p) or restart
   the process (stub) so the sanity check can reinstall the correct overlay and
   rehydrate peer state.

A green sanity check plus non-zero overlay gauges indicates the overlay swap is
operating correctly.

## Partition Detection

`net::partition_watch` tracks peer reachability and emits `partition_events_total`
when split-brain conditions arise. Gossip headers include the active partition
marker so peers can reconcile divergent histories. Recovery procedures and
metrics are documented in [network_partitions.md](network_partitions.md).

## QUIC Configuration

Enable QUIC with the `--quic` flag and expose a UDP port. Startup now reads
`config/default.toml` and merges any overrides from `config/quic.toml` into a
`transport::Config` before instantiating the provider registry. The new config
file carries the shared fields exposed by the transport layer:

* `provider` — set to `"quinn"` or `"s2n-quic"` to select the backend.
* `certificate_cache` — optional on-disk cache used by providers that manage
  X.509 material (s2n-quic honours the path directly; Quinn uses it when
  mirroring fingerprints for peers).
* `retry_attempts` / `retry_backoff_ms` — socket bind/connection retry policy
  forwarded to the active backend.
* `handshake_timeout_ms` — connection handshake deadline (default 5 seconds).
* `rotation_history` / `rotation_max_age_secs` — certificate persistence
  policy applied per provider. Defaults retain four historical fingerprints for
  30 days.

The registry defaults to Quinn when the file is absent, and the loader applies
the selected rotation policy on both initial startup and every config reload so
changes take effect without restarts.

Every QUIC handshake now advertises the provider identifier and the capability
set reported by the backend (`certificate_rotation`, `telemetry_callbacks`, etc.).
The handshake layer persists these fields in peer state, and both the CLI and
RPC endpoints surface them so operators can see which peers have migrated to a
different implementation. Successful connections increment
`quic_provider_connect_total{provider}`, allowing dashboards to watch provider
mix changes over time. The telemetry callbacks remain replaceable and continue
to drive the existing latency, retransmit, and disconnect metrics regardless of
the selected backend.

With the Quinn provider the transport layer derives an ephemeral X.509
certificate from the node’s Ed25519 network key, rotates out any certificate
older than the configured TTL, and persists the previous chain so peers can
migrate without drops. Gossip messages include the active fingerprint, and
`p2p::handshake` verifies that the presented certificate matches both the
gossiped fingerprint and the signer identity. A per-peer cache stored at
`~/.the_block/quic_peer_certs.json` lets the node reject stale or spoofed
fingerprints; the same cache backs the explorer endpoint at `/network/certs`.

Selecting the s2n provider honours the same configuration and telemetry hooks
while deriving certificates from the shared cache path. Certificate history is
persisted per provider so switching backends does not overwrite Quinn’s stored
fingerprints or the telemetry served by `net.quic_certs`.

Manual rotations are available via `blockctl net quic rotate` (or the
equivalent RPC), which returns the new fingerprint along with the previous
chain for audit logging. Every successful rotation increments
`quic_cert_rotation_total{peer}` for dashboards, tagging the rotating peer so
operators can correlate the event with transport metrics. Session health is
exposed through the `net.quic_stats` RPC: operators receive cached latency,
retransmit totals, endpoint reuse counts, and per-peer
`quic_handshake_fail_total{peer}` values. The CLI wrapper
`blockctl net quic stats --json --token <AUTH>` renders the same data for
scripting, while the aggregator can trigger automated log dumps whenever a
peer’s failure counter spikes.

`net.quic_certs` surfaces the full certificate cache—including the active
fingerprint, prior history, observed age, and whether the node still retains the
DER blob—for dashboard automation. The CLI mirrors this via
`blockctl net quic history`, which prints a human readable summary or JSON.
Use `blockctl net quic refresh` (RPC: `net.quic_certs_refresh`) when
rotations occur out-of-band; the node will reload the cache immediately instead
of waiting for the filesystem watcher to notice the update. History entries are
age-bounded (`MAX_PEER_CERT_HISTORY` × 30 days) so stale fingerprints are
pruned automatically before persistence. Entries are keyed by provider
identifier, and the CLI reports rotation timestamps per backend so mixed Quinn
and s2n deployments remain auditable during migrations.

Certificate blobs are encrypted on disk using a ChaCha20-Poly1305 key derived
from the node’s Ed25519 signing key (or `TB_PEER_CERT_KEY_HEX`). Operators
running stateless deployments can disable persistence entirely by exporting
`TB_PEER_CERT_DISABLE_DISK=1`, in which case the cache remains in-memory and no
filesystem writes occur. Incoming certificates are hashed before persistence to
guard against corrupted disk entries, and the background watcher hot-reloads the
cache whenever `quic_peer_certs.json` changes so long-lived nodes never serve
stale fingerprints.

Metrics `quic_conn_latency_seconds`, `quic_bytes_sent_total`,
`quic_bytes_recv_total`, `quic_retransmit_total`, and
`quic_endpoint_reuse_total` capture transport-level behaviour. Handshake issues
are split across `quic_handshake_fail_total{peer}` (peer-visible failures) and
`handshake_fail_total{reason}` (local reasons). The node regenerates its
certificate chain automatically when files are missing, ownership is incorrect,
or the TTL elapses, ensuring rotation without manual cleanup.

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
TCP. Each failure increments `quic_handshake_fail_total{peer}` and is reflected
in the cached diagnostics surfaced by `blockctl net quic stats`. A spike in
these counters usually signals certificate drift, UDP filtering, or exhausted
token buckets. Pair the CLI output with the metrics-to-logs correlation tool to
pull the associated log dumps automatically; the aggregator requests dumps when
`quic_handshake_fail_total{peer}` climbs faster than expected. When fallback
occurs the gossip message continues over the established TCP channel, so
functionality is preserved while operators investigate the root cause.

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
the `net stats` CLI:

```bash
blockctl net stats <peer_id>
```

The CLI accepts several flags:

| Flag | Purpose |
|------|---------|
| `--format <table|json>` | Output style (`table` is default). |
| `--drop-reason <reason>` | Show peers with drops matching the given reason. |
| `--min-reputation <f64>` | Include only peers with reputation ≥ threshold. |
| `--all` | List every tracked peer with pagination. |
| `--limit <n>` | Page size when using `--all` (default 50). |
| `--offset <n>` | Starting index when paging with `--all`. |

Drop rates ≥5 % render in yellow and ≥20 % in red. The command exits with `0`
on success, `2` when a peer is unknown, and `3` if access is unauthorized.

Examples:

```bash
# JSON output filtered by drop reason and reputation
blockctl net stats --drop-reason throttle --min-reputation 0.5 --format json

# Paginate through the full peer set
blockctl net stats --all --limit 25 --offset 25
```

Each query increments `peer_stats_query_total{peer_id}`. Results honour
`peer_metrics_export` and `max_peer_metrics` configuration caps. For bulk
inspection across the cluster, nodes can ship snapshots to the
`metrics-aggregator` service.

Use `net.peer_stats_all` or the CLI with `--all` to fetch paginated results.
When `--all` is issued without explicit paging flags, the command pauses after
each page; press <kbd>Enter</kbd> to advance. See
[docs/gossip.md](gossip.md) for protocol semantics and
[docs/operators/run_a_node.md](operators/run_a_node.md) for operator workflows.

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

The output includes `throttle=<reason>` while a peer is throttled. List all
currently throttled peers with:

```bash
net stats --backpressure
```

Operators can manually engage or clear throttling:

```bash
net stats throttle <peer_id>
net stats throttle <peer_id> --clear
```

Each action increments the `peer_throttle_total{reason}` counter for
observability. Cleared throttles also register under
`peer_backpressure_active_total{reason}` and dropped requests under
`peer_backpressure_dropped_total{reason}`.

For deterministic tests, setting `TB_PEER_SEED=<u64>` fixes the shuffle order
returned by `PeerSet::bootstrap`. This allows reproducible bootstrap sequences
when running integration tests or chaos simulations.

## RPC Client Timeouts

RPC clients stagger their request retries to avoid thundering herds.  The
following environment variables control timeout behaviour:

- `TB_RPC_TIMEOUT_MS` – base timeout per request (default 5000ms)
- `TB_RPC_TIMEOUT_JITTER_MS` – additional random jitter added to each timeout
  (default 1000ms)
- `TB_RPC_MAX_RETRIES` – number of retry attempts on timeout (default 3).
  The exponential multiplier saturates at `2^30` once retries reach 31
  (`MAX_BACKOFF_EXPONENT` in [`node/src/rpc/client.rs`](../node/src/rpc/client.rs)),
  so later attempts reuse that multiplier while keeping jitter in the delay
  calculation.
- `TB_RPC_FAULT_RATE` – probability for chaos-induced faults. Values outside
  the inclusive `[0.0, 1.0]` range are clamped and `NaN` inputs are ignored so
  the client never panics when chaos testing is enabled.

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

## Appendix · Runtime Socket Examples

The runtime facade exposes first-party TCP/UDP primitives so node components,
CLI tools, and tests can share the same async networking layer regardless of
backend. The API mirrors familiar Tokio ergonomics while remaining backend
agnostic.

### TCP server accepting JSON-RPC requests

```rust
use runtime::net::TcpListener;
use runtime::io::BufferedTcpStream;
use runtime::spawn;
use std::net::SocketAddr;

async fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, _peer) = listener.accept().await?;
        spawn(async move {
            let mut framed = BufferedTcpStream::new(stream);
            let mut line = String::new();
            if framed.read_line(&mut line).await.is_ok() {
                // handle request line and body reads here
            }
        });
    }
}
```

`BufferedTcpStream` keeps any bytes read ahead during header parsing so body
reads can reuse the buffer without additional syscalls. The helper behaves like
`runtime::io::BufReader` but is implemented entirely on top of the in-house
socket layer.

### Length-prefixed framing

Use the framing helpers when exchanging binary messages. Frames are encoded as a
big-endian u32 length followed by the payload and integrate with the buffered
reader for efficiency.

```rust
use runtime::io::{read_length_prefixed, write_length_prefixed};
use runtime::net::TcpStream;

async fn echo(mut stream: TcpStream) -> std::io::Result<()> {
    while let Some(frame) = read_length_prefixed(&mut stream, 64 * 1024).await? {
        write_length_prefixed(&mut stream, &frame).await?;
    }
    Ok(())
}
```

`read_length_prefixed` returns `Ok(None)` when the peer closes the connection
cleanly. Both helpers enforce size limits and surface `UnexpectedEof` errors
when a peer truncates a frame.

### UDP round trips

```rust
use runtime::net::UdpSocket;
use std::net::SocketAddr;

async fn udp_echo() -> std::io::Result<()> {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let mut server = UdpSocket::bind(addr).await?;
    let server_addr = server.local_addr()?;

    let client = UdpSocket::bind("127.0.0.1:0".parse().unwrap()).await?;
    client.send_to(b"ping", server_addr).await?;

    let mut buf = [0u8; 8];
    let (len, peer) = server.recv_from(&mut buf).await?;
    server.send_to(&buf[..len], peer).await?;
    Ok(())
}
```

The sockets register with the in-house reactor automatically and work across all
compiled backends (`inhouse` or the stub fallback). These examples
double as smoke tests when developing new runtime backends or adjusting the
polling integration.
