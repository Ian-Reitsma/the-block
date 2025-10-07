# Integration and Chaos Testing
> **Review (2025-10-01):** Captured first-party Ed25519 backend requirement across CLI integration harnesses.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This document outlines the strategy for integration and chaos testing in **The-Block**.

> **Current caveats (2025-10-01):** CLI binaries now build cleanly on the
> crypto suite’s first-party Ed25519 backend, the
> transport crate re-exports the `quic` feature so
> integration harnesses must pass `--features "integration-tests quic"` when
> selecting providers, and telemetry-gated modules still emit warnings when
> optional features are disabled. Prefer the `lightweight-integration` feature
> set for memory-constrained runs and re-enable full telemetry when validating
> release candidates.

## First-party testkit macros

The third-party benchmarking, property-testing, snapshot, and serialisation
harnesses have been replaced with the first-party `testkit` crate. Developers
should use the helper macros below when authoring or migrating tests:

- `tb_bench!` wraps former Criterion benchmarks and now executes the body a
  deterministic number of iterations (100 by default). The harness records
  total and per-iteration timings and prints them to STDOUT so runs can be
  compared without external tooling. Pass `iterations = <count>` to override
  the loop count for heavyweight benchmarks.
- `tb_prop_test!` replaces `proptest!` blocks. The macro exposes a
  [`prop::Runner`](../crates/testkit/src/lib.rs) that supports deterministic
  cases via `add_case` and pseudo-random coverage via `add_random_case`. The
  built-in PRNG honours the optional `TB_PROP_SEED` environment variable, making
  failing scenarios reproducible without third-party engines.
- `tb_snapshot_test!` and `tb_snapshot!` replace Insta-style assertions. Values
  are compared against UTF-8 snapshots stored under `tests/snapshots/<module>`,
  and setting `TB_UPDATE_SNAPSHOTS=1` rewrites the on-disk baseline. The helper
  normalises line endings so recordings are stable across platforms.
- `tb_fixture!` declares reusable fixtures that return a lightweight wrapper
  implementing `Deref`/`DerefMut`, enabling explicit teardown without a global
  registry.
- `tb_serial` (provided by `testkit_macros`) enforces serial execution by
  locking a global mutex inside the generated `#[test]`, preventing concurrent
  access to shared resources during integration runs.

These macros now execute real harness code; property suites, benchmarks, and
snapshots should be maintained alongside the production stack instead of relying
on external crates. Document any remaining manual coverage expectations in code
reviews when behaviour changes.

## Network Integration

- `node/tests/net_integration.rs` boots a five-node harness that mines blocks,
  partitions the network into two groups, and then reconnects to verify that
  fork resolution selects the longest chain.

## Governance Upgrades

- `node/tests/gov_upgrade.rs` exports a governance snapshot and ensures the
  artifact contains expected fields. This verifies that upgrade templates are
  stable while parameters are updated mid-run.

## Storage Chaos

- `node/tests/storage_chaos.rs` deletes RocksDB WAL files after writes using a
  deterministic RNG seed and confirms the database can recover without data loss.

## Concurrent Market Stress

- `node/tests/concurrent_markets.rs` runs DEX swaps and compute market price
  band calculations on separate threads to surface race conditions.

## Long-Running Test Harness

- `scripts/longtest.sh` repeatedly runs the full test suite while capturing
  cluster metrics to `longtest-metrics.prom`. Pass a custom duration in seconds
  to control the run length (12 hours by default).

## Deterministic Seeds

- Integration tests use explicit `StdRng::seed_from_u64` seeds or the
  `TB_SIM_SEED` environment variable to enable reproducible scenarios.

## RPC Fault Injection and Retry Backoff

- `node/src/rpc/client.rs` accepts `TB_RPC_FAULT_RATE` to randomly inject
  request failures. Values are sanitized by clamping the probability to the
  inclusive `[0.0, 1.0]` range and ignoring `NaN` inputs. The regression tests
  in `node/tests/rpc_fault_injection.rs` verify that the client surfaces these
  errors for resiliency testing.
- `rpc_client_backoff_handles_large_retries` exercises the saturated exponential
  backoff path, confirming that attempts above the 31st reuse the `2^30`
  multiplier rather than overflowing. The helper test `backoff_with_jitter_saturates_for_large_attempts`
  keeps the original monotonicity assertions in place.

Run the targeted retry and fault sanitization suites directly when touching the
client:

```bash
cargo test -p the_block --lib rpc_client_backoff_handles_large_retries -- --nocapture
cargo test -p the_block --lib rpc_client_fault_rate_clamping -- --nocapture
```

## Coverage Reporting

- `.github/workflows/coverage.yml` runs Tarpaulin in CI and publishes an XML
  coverage report as an artifact.

## Running Tests

```
cargo test --all --features test-telemetry --release
```

### Memory-Constrained integration runs

- The `lightweight-integration` crate feature swaps the legacy
  RocksDB-compatible adapter for an in-memory implementation that snapshots
  column families to disk. Enable this feature when running the heaviest
  integration targets (such as `gov_dependencies` and `maybe_purge_loop`) on
  machines with limited RAM:

  ```
  cargo test -p the_block --no-default-features --features lightweight-integration --test gov_dependencies
  cargo test -p the_block --no-default-features --features lightweight-integration --test maybe_purge_loop
  ```

- To compare behaviour with the compatibility wrapper, run the same targets
  with `--features storage-rocksdb` (and optionally `--no-default-features` if
  you only need the library). The flag now maps to the first-party engine while
  preserving historical feature gating:

  ```
  cargo test -p the_block --no-default-features --features storage-rocksdb --test gov_dependencies
  ```

- `cargo test` without additional flags now exercises the first-party backend.
  The RocksDB stack is no longer linked during workspace builds.

### Opting in to integration binaries

- The heavy integration harnesses under `node/tests/` are now gated behind the
  `integration-tests` Cargo feature. Enable the flag when you need the full
  end-to-end coverage:

  ```
  cargo test -p the_block --features integration-tests
  ```

  Targeted runs accept the same feature, for example:

  ```
  cargo test -p the_block --features integration-tests --test light_sync -- --nocapture
  ```

- CLI binaries (`node`, `gov`, `wallet`, etc.) are no longer compiled during
  routine `cargo test` invocations. Add `--features cli` to build them when a
  test requires spawning the command-line tools or when linting the binaries:

  ```
  cargo build -p the_block --features cli --bin node
  ```
