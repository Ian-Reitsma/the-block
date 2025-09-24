# Light-Client Synchronization Guide
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

*Last updated: 2025-09-23*

The `light-client` crate offers a minimal header verifier intended for mobile
and resource-constrained environments. It trades full validation for a compact
sync process that still detects blatant forks and stale peers.

## Sync Options

`SyncOptions` gate background synchronization based on device conditions and
drive batching behaviour:

```rust
pub struct SyncOptions {
    pub wifi_only: bool,
    pub require_charging: bool,
    pub min_battery: f32,
    pub batch_size: usize,
    pub poll_interval: Duration,
    pub stale_after: Duration,
    pub fallback: DeviceFallback,
}
```

`SyncOptions::default()` requires Wi‑Fi, a charging device, and at least 50%
battery. The new `batch_size` and `poll_interval` fields bound the amount of
work processed per loop and how often device conditions are re-polled, while
`stale_after` controls how long cached probe results remain valid before the
fallback policy is used. `DeviceFallback` captures the behaviour when probes are
unavailable (e.g. desktop builds).

`sync_background(client, opts, fetch)` is now `async` and returns a
`Result<SyncOutcome, ProbeError>`. Each iteration requests at most
`batch_size` headers via the `fetch(start_height, batch_size)` closure, verifies
them, and re-evaluates the device status before continuing. If Wi‑Fi drops,
charging stops, or the battery sinks below `min_battery`, the function stops and
reports the gating reason inside `SyncOutcome::gating`.

Device conditions are provided by implementations of the `DeviceStatusProbe`
trait. The crate ships Android and iOS probes behind feature flags, plus a
desktop fallback that always defers to the configured `DeviceFallback`. Probes
are polled asynchronously and cached for `stale_after`; if the backend fails,
the last known status (or the fallback policy) is used while emitting a
`tracing` warning. Every probe result is reported to the
`light_client_device_status` Prometheus gauge (full metric
`the_block_light_client_device_status{field="wifi|charging|battery",freshness="fresh|cached|fallback"}`) when the
`telemetry` feature is enabled. `DeviceStatusWatcher` records both the
monotonic timestamp and wall-clock instant for each poll so cached reads expose
how stale the data is; freshness labels let dashboards differentiate a healthy
backend from fallback policy. The CLI/RPC surfaces now echo the last probed
status so operators see why sync paused.

## Header Verification

Headers are represented by the simplified `Header` struct. `LightClient` keeps a
vector of headers and appends new ones after basic height checks:

```rust
pub fn verify_and_append(&mut self, h: Header) -> Result<(), ()> {
    // Verifies previous hash linkage, PoW difficulty and optional checkpoints
    // before appending.
}
```

A production client must additionally verify:

- BLAKE3 proof-of-work meets the advertised difficulty target.
- The previous hash matches the tip of the local chain.
- Validator signatures or finality proofs if operating in PoS mode.
- Checkpoint headers sourced from trusted channels.

### Checkpoint Invalidation

Trusted checkpoints can be revoked by governance in the event of a detected
fork. Clients must track checkpoint hashes by height and refuse headers whose
`checkpoint_hash` no longer matches the trusted list. When a checkpoint is
invalidated, the light client should roll back to the last valid height and
re-sync from that point to avoid following an obsolete chain.

### Header Cache Rules

Mobile deployments retain only a sliding window of recent headers to conserve
storage. Older entries beyond a few thousand blocks may be evicted, but any
height anchoring a trusted checkpoint must remain cached until the checkpoint
expires or is explicitly revoked. Cache eviction must never drop a header that
is still required to verify PoW linkage from the last checkpoint.

## Security Model

The demo implementation assumes a trusted bootstrap header.  Without cryptographic
proofs, an adversary could feed arbitrary headers.  Real deployments should ship
hard-coded checkpoints and verify cumulative work or stake before accepting
updates.

Because light clients only track headers, they rely on full nodes for transaction
proofs.  Any application using the light client must validate Merkle proofs for
account state or receipts before acting on them.

## Usage Example

The workspace's `runtime` crate drives asynchronous helpers so examples remain backend-agnostic:

```rust
use light_client::{Header, LightClient, SyncOptions, sync_background};

let genesis = Header { height: 0, ..Default::default() };
let mut lc = LightClient::new(genesis);
let opts = SyncOptions::default();
let outcome = runtime::block_on(async {
    sync_background(&mut lc, opts, |_start, _batch| Vec::new()).await
})?;
if let Some(reason) = outcome.gating {
    eprintln!("sync gated: {}", reason.as_str());
}
```

## Device policy

Mobile builds persist operator overrides in `~/.the_block/light_client.toml`.
`LightClientConfig` exposes `ignore_charging_requirement`, optional Wi‑Fi and
minimum battery overrides, and a custom `DeviceFallback`. Call
`SyncOptions::apply_config` to merge disk settings with runtime policy:

```rust
let cfg = light_client::load_user_config()?;
let opts = SyncOptions::default().apply_config(&cfg);
```

CLI users can toggle the charging requirement via `contract light-client device set --ignore-charging true`, inspect the
live probe snapshot with `contract light-client device show`, and reset to defaults with
`contract light-client device reset`. The crate ensures the configuration directory is created on first write and
validates values so operators cannot persist out-of-range battery thresholds. RPC consumers can inspect the same gating
message through `light_client.device_status`. The same device state is embedded in log uploads via
`upload_compressed_logs`, which now returns an `AnnotatedLogBundle` containing the compressed payload alongside a
serialized snapshot of the last probed status, freshness label, and elapsed seconds since observation.

## Further Reading

- [`crates/light-client/src/lib.rs`](../crates/light-client/src/lib.rs) — source
  code with stubs to be replaced by platform integrations.
- [`docs/mobile_light_client.md`](mobile_light_client.md) — mobile-specific notes
  and background sync strategies.