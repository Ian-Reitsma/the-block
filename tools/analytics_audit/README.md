# analytics_audit
Guidance aligns with the dependency-sovereignty pivot; runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced.

Utility to verify `ReadAck` batches against on-chain totals.

```bash
cargo run -p analytics_audit -- path/to/batch.bin
```

The tool recomputes the Merkle root and total bytes of a first-party
binary batch file and prints them for comparison with the `read_root`
and `read_sub` fields recorded in the corresponding block.
