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

## Push Notifications

Mobile clients can opt in to credit balance and rate-limit notifications by
registering a webhook endpoint:

```rust
use wallet::CreditNotifier;

let mut notifier = CreditNotifier::default();
notifier.register_webhook("https://example.com/push");
notifier.notify_balance_change("provider-id", 42)?;
```

The notifier posts JSON payloads to each endpoint when balances change or when
the client hits RPC rate limits.  Applications can forward these events to the
platform's native push service (FCM/APNS) to alert users.

## Sample Apps

Example iOS and Android projects demonstrate invoking the KYC flow and
reporting contribution metrics gathered during background sync.
