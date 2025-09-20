# Mobile Light-Client Guidance

The light client can opportunistically sync in the background when the device is
plugged in, on Wi‑Fi, and above a configurable battery threshold. The sync loop
is exposed via the `light_client` crate and can be embedded in native apps.

## Background Sync

```rust
use light_client::{LightClient, SyncOptions, sync_background, Header};

let genesis = Header::default();
let mut client = LightClient::new(genesis);
let opts = SyncOptions::default();
sync_background(&mut client, opts, |_start| Vec::new());
```

The helper checks platform power and connectivity hints before contacting the
network, conserving user resources. The default options require Wi‑Fi,
charging, and at least 50 % battery; adjust the struct fields to loosen those
guards for debug builds.

### Compressed Log Uploads

Logs generated during background sync can be compressed with
`light_client::upload_compressed_logs` before being sent to the telemetry
collector, reducing bandwidth for mobile clients.

## SDKs

Swift and Kotlin bindings are provided under `examples/mobile/` and wrap the
above API. They allow wallets to integrate light-client features without
resorting to JNI or bridging layers.

## Sample Apps

Example iOS and Android projects demonstrate invoking the KYC flow and
reporting contribution metrics gathered during background sync.
