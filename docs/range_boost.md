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

Telemetry records:

- `mesh_peer_connected_total{peer_id}` – total mesh peers discovered.
- `mesh_peer_latency_ms{peer_id}` – last observed latency in milliseconds.

`node/tests/mesh_sim.rs` provides a UNIX-domain-socket harness that simulates
mesh links and validates latency-based scoring.

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
