# Write-Ahead Log Fuzzing

The `tests/wal_fuzz.rs` harness exercises crash recovery by generating random write-ahead log entries with the `arbitrary` crate. WAL files are truncated at random offsets to simulate crashes, and the database is reopened to ensure the replayed state matches the expected account balances.

## How CI runs

A nightly job runs `cargo +nightly fuzz run wal_fuzz -- -max_total_time=120 -artifact_prefix=fuzz/wal/ -runs=0`. Any files left under `fuzz/wal/` are uploaded as artifacts and cause the job to fail, gating PR merge.

## What artifacts mean

`fuzz/wal/` stores both crashing inputs and interesting seeds. An empty directory means the fuzz pass was clean. If files appear, examine them before fixing the bug or promoting the seed.

## Replaying a crash

To replay the most recent artifact locally:

```bash
scripts/triage_wal.sh
```

`triage_wal.sh` re-runs the fuzz target with `RUST_BACKTRACE=1` so you get a stack trace.

## Promoting a seed

1. List artifacts and seeds:
   ```bash
   scripts/extract_wal_seeds.sh fuzz/wal
   ```
2. Copy a promising file into `fuzz/corpus/wal/` and commit it.
3. Document any notable failures in this file under *Failure Triage*.

## Local longer run

For a deeper soak, run:

```bash
cargo +nightly fuzz run wal_fuzz -- -max_total_time=1800 -artifact_prefix=fuzz/wal/ -runs=0
```

## Sanitizers

For local debugging you can enable AddressSanitizer (skipped in CI to keep it fast):

```bash
RUSTFLAGS="-Zsanitizer=address" cargo +nightly fuzz run wal_fuzz -- -max_total_time=60 -artifact_prefix=fuzz/wal/ -runs=0
```

## Failure Triage

1. Minimize the crashing input with `cargo fuzz tmin wal_fuzz crash-<hash>`.
2. Record the RNG seed from `scripts/extract_wal_seeds.sh` and include it in the issue report.
3. Add a regression test that reproduces the failure deterministically.

Summaries of notable failures and their seeds live below and should be kept current:

| Pattern | Seed | Reproduction |
|--------|------|--------------|
| *(none reported)* | – | – |

## Compaction Crash Recovery

During log compaction, a crash after writing the data file but before the WAL
is rotated can leave a fully applied record in both places. The
`wal_replays_once_after_compaction_crash` test ensures that on restart the log
is replayed exactly once and then removed, so reopened databases see no
duplicate entries.

## End-of-Compaction Marker

Each flush appends an `End` marker carrying the last applied id to the WAL
before the file is removed. If a crash happens after this marker is written but
before deletion, startup detects the marker and discards the log without
replaying it. The database also tracks a monotonic id, skipping any WAL records
that were already persisted to the main data file.
