# Write-Ahead Log Fuzzing

The `tests/wal_fuzz.rs` harness exercises crash recovery by generating random
write-ahead log entries with the `arbitrary` crate. WAL files are truncated at
random offsets to simulate crashes, and the database is reopened to ensure the
replayed state matches the expected account balances.

For deeper coverage, run the libFuzzer target:

```bash
make fuzz-wal # runs `cargo fuzz run wal_fuzz --max-total-time=60 -- -artifact_prefix=fuzz/wal/`
```

`cargo fuzz` requires the Rust *nightly* toolchain. The CI job installs
nightly automatically, but local developers may need to run
`rustup toolchain install nightly` beforehand.

Artifacts from `cargo fuzz` are retained under `fuzz/wal/` along with the RNG
seed for deterministic reproduction. To reproduce a failing case:

```bash
cargo fuzz run wal_fuzz -- -seed=<seed> fuzz/wal/<file>
```

List collected seeds with the helper script:

```bash
scripts/extract_wal_seeds.sh fuzz/wal
```

Known failure signatures:

- checksum mismatch indicates a torn WAL entry
- replay divergence where recovered balances differ from expected

## Failure Triage

1. Minimize the crashing input with `cargo fuzz tmin wal_fuzz crash-<hash>`.
2. Record the RNG seed from `scripts/extract_wal_seeds.sh` and include it in
   the issue report.
3. Add a regression test that reproduces the failure deterministically.

Summaries of notable failures and their seeds live below and should be kept
current:

| Pattern | Seed | Reproduction |
|--------|------|--------------|
| *(none reported)* | – | – |
