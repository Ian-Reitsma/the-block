# Swift Sample

```swift
import LightClient

let opts = SyncOptions(wifiOnly: true, requireCharging: true, minBattery: 0.5)
LightClient.syncBackground(opts)

// Trigger optional KYC verification
Rpc.call(method: "kyc.verify", params: ["user": "alice"]) { result in
    print(result)
}
```
