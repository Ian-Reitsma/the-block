# Storage Repair Overlay
> **Review (2025-09-25):** Synced Storage Repair Overlay guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

To sustain blob recovery over low-bandwidth BLE links, the repair path uses a
RaptorQ fountain overlay tuned with the following constants:

```
chunk size   : 4 MiB
symbol size  : 1 KiB
rate         : 1.2 (20% overhead)
```

At this rate a 4 GB object requires no more than 25 MB of extra symbols while
achieving a 99 % success probability for reconstructing any single missing
shard.

The `raptorq_repair_roundtrip` helper in `storage::repair` exercises this
configuration and is used in tests to verify recovery after a simulated
single‑shard loss.
