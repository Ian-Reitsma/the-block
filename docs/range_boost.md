# Range-Boost Store-and-Forward Queue
> **Review (2025-09-25):** Synced Range-Boost Store-and-Forward Queue guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

`RangeBoost` enables delay-tolerant networking by queueing bundles of payloads
and recording cryptographic hop proofs as they traverse intermittent relays.
This mechanism allows mobile or offline nodes to ferry data until they reconnect
to the wider network.

## Bundle Format

A bundle contains a raw `payload` plus an ordered list of `HopProof` records
identifying relays that handled it:

```rust
pub struct HopProof { pub relay: String }
pub struct Bundle   { pub payload: Vec<u8>, pub proofs: Vec<HopProof> }
```

Bundles are enqueued with [`enqueue`](../node/src/range_boost/mod.rs#L25-L30),
which stores the payload and an empty proof list. As the bundle moves through
relays, each hop records its participation via [`record_proof`](../node/src/range_boost/mod.rs#L32-L35).
When the destination receives the bundle, it calls [`dequeue`](../node/src/range_boost/mod.rs#L38-L40)
to remove the oldest entry and process it. [`pending`](../node/src/range_boost/mod.rs#L42-L43)
returns the current queue depth for monitoring.

## Use Cases

- **Offline relays** – Smartphones or vehicular nodes can collect data while
offline and forward it once connectivity resumes, appending their identity as a
hop proof.
- **Long-range mesh** – Nodes beyond direct radio range can leverage passerby
  devices to hop bundles toward the core network.
- **Ad delivery enrichment** – The HTTP gateway consults `RangeBoost::best_peer()`
  when an impression requests mesh delivery, embedding the peer identifier,
  inferred transport, and observed latency into the `ReadAck` so advertisers can
  target or audit mesh impressions without guessing which hop carried the
  payload. Eligible creatives are simultaneously staged into the
  first-party queue via `RangeBoost::enqueue`, and mesh responses append hop
  proofs as relays forward bundles, keeping Bluetooth/Wi-Fi distribution
  observable without third-party tooling.

## Operational Notes

1. **Payload sizing** – The reference tests use 4‑byte payloads but real
deployments should keep bundles small (≤1 MiB) to minimize storage pressure on
relays.
2. **Proof integrity** – Each `HopProof` currently records only a relay string.
Future revisions may add signatures to prevent spoofing.
3. **Queue monitoring** – Operators should track `pending()` depth and add
expiry timestamps to drop stale bundles once telemetry hooks are available.
4. **Gossip integration** – Batches can be advertised over the gossip layer to
neighbouring nodes for faster dissemination.
5. **Testing** – `node/tests/range_boost.rs` demonstrates enqueueing, proof
recording, and dequeue semantics.

## Forwarder runtime

`RangeBoost::spawn_forwarder` now owns the queue-draining worker. The gateway
instantiates the queue on startup and only spawns the forwarder when
`range_boost::is_enabled()` is true (the `--range-boost` CLI flag or
`TB_MESH_STATIC_PEERS` environment variable). Nodes that run strictly over HTTP
therefore skip the background thread entirely, avoiding needless wake-ups. When
mesh mode is active the worker upgrades a weak reference to the queue, pops the
oldest bundle, and forwards it to the lowest-latency peer discovered by
`best_peer()`. Failed deliveries are requeued and logged via
`diagnostics::log::{info!, warn!}` so operators can spot unsupported transports
or transient sockets without third-party tooling. The loop sleeps while the
queue is empty or mesh mode is disabled, and exits automatically if the owning
`Arc` drops (e.g., during shutdown or tests).

Runtime toggling now rides `RangeBoost::set_enabled`. The queue keeps a
`ForwarderState` so flipping the flag wakes the worker immediately. When mesh
mode is disabled the thread drains in-flight bundles, commits any retry state,
and then parks without spinning. Re-enabling mesh delivery reuses the same
thread where possible and only spawns a replacement if shutdown already
completed, letting operators and tests switch delivery strategies without
restarting the node or leaking workers. The `node/tests/mesh_sim.rs` harness
exercises rapid enable/disable loops to guarantee queued payloads survive while
the worker is paused.

## Failure injection and stress testing

The queue now exposes first-party fault hooks so stress suites can flip failure
paths without external harnesses:

- `range_boost::set_forwarder_fault_mode(FaultMode::ForceEncode | ForceIo |
  ForceNoPeers | ForceDisabled)` forces the next `forward_bundle` to surface
  the requested error, exercising retry branches and logging without altering
  production code.
- `range_boost::inject_enqueue_error()` drops the next `enqueue` attempt and
  increments the dedicated telemetry counter so operators can validate alerting
  without staging malformed payloads.

`node/tests/range_boost.rs` gained the `range_boost_fault_injection_counts_failures`
and `range_boost_toggle_latency_records_histogram` suites, hammering rapid
enable/disable cycles while asserting the new telemetry surfaces. The forwarder
tests stay entirely inside the in-tree queue and concurrency primitives—no
third-party mocks or mesh toolkits are required.

## Telemetry

RangeBoost emits both peer-discovery gauges and queue-level instrumentation:

- `mesh_peer_connected_total{peer_id}` – total mesh peers discovered.
- `mesh_peer_latency_ms{peer_id}` – last observed latency in milliseconds.
- `range_boost_forwarder_fail_total` – cumulative forwarder errors (retry
  paths, injected failures, unsupported transports).
- `range_boost_enqueue_error_total` – number of intentionally dropped enqueues
  (via the injection hook) so dashboards can confirm alert wiring.
- `range_boost_toggle_latency_seconds` – histogram tracking the interval
  between enable/disable calls, giving operators confidence that toggles reach
  the worker without stalling.

These counters land in the existing Prometheus registry and back the new
`test-range-boost` Justfile recipe, letting CI exercise mesh-mode telemetry
without compiling auxiliary tooling. Grafana now exposes a **Range Boost** row
for the forwarder/error/toggle metrics, and the
`RangeBoostForwarderFailures` alert pages when retries accumulate without
operators intentionally flipping the mesh toggle.

## Example

```rust
use the_block::range_boost::{RangeBoost, HopProof};

let mut rb = RangeBoost::new();
rb.enqueue(b"hello".to_vec());
rb.record_proof(0, HopProof { relay: "peer1".into() });
let bundle = rb.dequeue().unwrap();
assert_eq!(bundle.proofs[0].relay, "peer1");
```

Range Boost provides the foundation for opportunistic mesh relays, with future
work planned for expiration, persistence, and incentivization.

## Local Mesh Networking

`discover_peers` performs Wi‑Fi and (on Linux/macOS when built with the
`bluetooth` feature) Bluetooth discovery. Addresses supplied via the
`TB_MESH_STATIC_PEERS` environment variable are probed and scored by round‑trip
latency. Gossip relays prefer low‑latency neighbours, and PoW mining yields CPU
time while mesh tasks are active to reduce contention.

Run the node with local mesh support using the `--range-boost` flag:

```shell
the-block node run --range-boost
```

Refer to the telemetry section above for the full metric list. The
`node/tests/mesh_sim.rs` harness still exercises peer-discovery gauges by
simulating UNIX-domain-socket transport and verifying latency-based scoring.

## Hardware & Setup

- **Bluetooth**: on Linux, install BlueZ utilities (`hcitool`) and ensure the
  adapter is enabled. Scanning is performed via `hcitool scan` when the node is
  launched with `--range-boost`.
- **Wi‑Fi**: the node invokes `iwlist scan` to enumerate nearby access points.
  Install `wireless-tools` and grant the process permission to query the
  interface.
- Mobile devices can experiment with mesh relays using the
  [`mobile_relay` example](../node/examples/mobile_relay.rs), which broadcasts a
  payload over a UNIX socket for integration tests.

Set `TB_MESH_STATIC_PEERS` to a comma-separated list of `unix:/path` or IP
endpoints to probe explicit neighbours.
