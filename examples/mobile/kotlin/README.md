# Kotlin Sample

```kotlin
import lightclient.SyncOptions
import lightclient.syncBackground

val opts = SyncOptions(true, true, 0.5f)
syncBackground(opts)

// Optional KYC verification via RPC
rpc.call("kyc.verify", mapOf("user" to "alice")) { result ->
    println(result)
}
```
