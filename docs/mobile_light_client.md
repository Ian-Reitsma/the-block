# Mobile Light-Client Guidance
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The light client can opportunistically sync in the background when the device is
plugged in, on Wi‑Fi, and above a configurable battery threshold. The sync loop
is exposed via the `light_client` crate and can be embedded in native apps.

## Background Sync

```rust
use light_client::{LightClient, SyncOptions, sync_background, Header};

let genesis = Header::default();
let mut client = LightClient::new(genesis);
let opts = SyncOptions::default();
let outcome = runtime::block_on(async {
    sync_background(&mut client, opts, |_start, _batch| Vec::new()).await
})?;
if let Some(reason) = outcome.gating {
    println!("background sync paused: {}", reason.as_str());
}
```

The helper checks platform power and connectivity hints before contacting the
network, conserving user resources. The default options require Wi‑Fi,
charging, and at least 50 % battery; adjust `SyncOptions` or the persisted
`LightClientConfig` to loosen those guards for debug builds. The function emits
per-probe telemetry under `the_block_light_client_device_status{field,freshness}`
(`field` ∈ {`wifi`,`charging`,`battery`}, `freshness` ∈ {`fresh`,`cached`,`fallback`}),
caches readings for the `stale_after` window, streams gating messages back to the CLI and RPC surfaces, and returns a
`SyncOutcome` describing how many headers were appended and why the loop exited.
`DeviceStatusWatcher` records the monotonic timestamp for each poll so cached readings include the elapsed age, making
it obvious when the implementation relied on a stale reading.

### Compressed Log Uploads

Logs generated during background sync can be compressed with
`light_client::upload_compressed_logs`. The helper now returns an
`AnnotatedLogBundle` containing both the compressed payload and the last device
status snapshot, including freshness labels and seconds since observation,
making it easy to attach context (Wi‑Fi, charging, battery level) to uploaded diagnostics.

## Configuration persistence

Mobile builds and the CLI persist overrides in `~/.the_block/light_client.toml`.
Set `ignore_charging_requirement = true` to sync on battery or adjust
`min_battery`/`wifi_only` thresholds for constrained test hardware. Use
`contract light-client device show` to inspect the current policy and the last
probed status, `contract light-client device set` to update thresholds, and
`contract light-client device reset` to restore defaults. The CLI surfaces validation errors if values fall outside
`[0.0, 1.0]`.

## SDKs

Swift and Kotlin bindings are provided under `examples/mobile/` and wrap the
above API. They allow wallets to integrate light-client features without
resorting to JNI or bridging layers.

## Sample Apps

Example iOS and Android projects demonstrate invoking the KYC flow and
reporting contribution metrics gathered during background sync.