# Write-Ahead Log Fuzzing

The `tests/wal_fuzz.rs` harness exercises crash recovery by generating random
write-ahead log entries with the `arbitrary` crate. WAL files are truncated at
random offsets to simulate crashes, and the database is reopened to ensure the
replayed state matches the expected account balances.

For deeper coverage, run the libFuzzer target:

```bash
make fuzz-wal # runs `cargo fuzz run wal_fuzz --max-iters=1000`
```

Artifacts from `cargo fuzz` are retained under `fuzz/wal/` for triage.

Known failure signatures:

- checksum mismatch indicates a torn WAL entry
- replay divergence where recovered balances differ from expected
