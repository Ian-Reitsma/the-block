# Mobile Light-Client Guidance

The light client can opportunistically sync in the background when the device is
plugged in, on Wi‑Fi, and above a configurable battery threshold. The sync loop
is exposed via the `light_client` crate and can be embedded in native apps.

## Background Sync

```rust
use light_client::{SyncOptions, sync_background};

let opts = SyncOptions { wifi_only: true, require_charging: true, min_battery: 0.3 };
sync_background(opts);
```

The helper checks platform power and connectivity hints before contacting the
network, conserving user resources.

## SDKs

Swift and Kotlin bindings are provided under `examples/mobile/` and wrap the
above API. They allow wallets to integrate light-client features without
resorting to JNI or bridging layers.

## Sample Apps

Example iOS and Android projects demonstrate invoking the KYC flow and
reporting contribution metrics gathered during background sync.
