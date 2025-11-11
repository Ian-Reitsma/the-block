# Kotlin Sample
Guidance aligns with the dependency-sovereignty pivot; runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced.

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
