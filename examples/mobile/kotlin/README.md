# Kotlin Sample
> **Review (2025-09-25):** Synced Kotlin Sample guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

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
