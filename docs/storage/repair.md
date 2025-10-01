# Storage Repair Overlay
> **Review (2025-09-30):** Updated repair overlay notes for the in-house LT fountain coder and helper rename.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

To sustain blob recovery over low-bandwidth BLE links, the repair path uses a
in-house LT fountain overlay tuned with the following constants:

```
chunk size   : 4 MiB
symbol size  : 1 KiB
rate         : 1.2 (20% overhead)
```

At this rate a 4 GB object requires no more than 25 MB of extra symbols while
achieving a 99 % success probability for reconstructing any single missing
shard.

The `fountain_repair_roundtrip` helper in `storage::repair` exercises this
configuration and is used in tests to verify recovery after a simulated
single‑shard loss.
