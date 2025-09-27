# analytics_audit
> **Review (2025-09-25):** Synced analytics_audit guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Utility to verify `ReadAck` batches against on-chain totals.

```bash
cargo run -p analytics_audit -- path/to/batch.cbor
```

The tool recomputes the Merkle root and total bytes of a CBOR batch file
and prints them for comparison with the `read_root` and `read_sub_ct`
fields recorded in the corresponding block.
