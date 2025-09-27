# Swift Sample
> **Review (2025-09-25):** Synced Swift Sample guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

```swift
import LightClient

let opts = SyncOptions(wifiOnly: true, requireCharging: true, minBattery: 0.5)
LightClient.syncBackground(opts)

// Trigger optional KYC verification
Rpc.call(method: "kyc.verify", params: ["user": "alice"]) { result in
    print(result)
}
```
