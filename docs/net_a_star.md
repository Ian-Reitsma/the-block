# ASN Latency Heuristic Routing
> **Review (2025-09-25):** Synced ASN Latency Heuristic Routing guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The networking stack exposes an A* router that steers connections toward peers
with low latency while avoiding unstable nodes.  The implementation lives in
[`node/src/net/a_star.rs`](../node/src/net/a_star.rs) and combines measured
Autonomous System Number (ASN) latency floors with peer uptime penalties.

## AsnLatencyCache

`AsnLatencyCache` tracks latency floors between ASN pairs.  It stores up to
`cap` entries in a least-recently-used map and recomputes measurements on
request:

- `get_or_insert(a, b, compute)` – returns a cached floor or invokes `compute`
  to measure a new one.
- `recompute(measure)` – refreshes every cached entry using the supplied
  measurement function.

Cache entries are symmetric—`(a, b)` and `(b, a)` share the same key—and are
used by the A* heuristic to estimate cost between peers.

## Heuristic Function

```rust
pub fn heuristic(cache: &mut AsnLatencyCache,
                 asn_src: u32,
                 asn_dst: u32,
                 uptime: f64,
                 mu: f64) -> f64
```

The heuristic adds the cached latency floor to an uptime penalty `mu *
(1.0 - uptime)`.  Higher `mu` values bias the search away from peers with poor
availability.

## Configuration

The router honours the following environment knobs:

- `TB_ASTAR_MAX_HOPS` – cap on path length explored during search (default 8).
- `TB_ASTAR_CACHE_TTL_MS` – age after which cached measurements are recomputed
  (default 3_600_000 ms).

Updating these variables at runtime lets operators trade accuracy for CPU
overhead.

## Metrics

When telemetry is enabled the router exposes:

- `asn_latency_ms{src, dst}` – last measured floor between two ASNs.
- `route_fail_total` – number of searches that failed to produce a path within
  the hop limit.

Metrics appear on the runtime telemetry exporter and can be scraped via
`curl localhost:9100/metrics | rg '^asn_latency_ms'`.

## Debugging

- **Cache misses** – ensure `TB_ASTAR_CACHE_TTL_MS` is large enough to avoid
  constant recomputation.  Logs tagged with `asn_cache_miss` indicate an empty or
  evicted entry.
- **Stale measurements** – call `AsnLatencyCache::recompute` periodically or
  restart the node to refresh the cache.

The `docs/networking.md` guide references this module under the routing and
latency section.
