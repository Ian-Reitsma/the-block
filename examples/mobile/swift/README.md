# Swift Sample
Guidance aligns with the dependency-sovereignty pivot; runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced.

```swift
import LightClient

let opts = SyncOptions(wifiOnly: true, requireCharging: true, minBattery: 0.5)
LightClient.syncBackground(opts)

// Trigger optional KYC verification
Rpc.call(method: "kyc.verify", params: ["user": "alice"]) { result in
    print(result)
}
```
