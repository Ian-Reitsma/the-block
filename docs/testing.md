# Integration and Chaos Testing
> **Review (2025-09-25):** Synced Integration and Chaos Testing guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This document outlines the strategy for integration and chaos testing in **The-Block**.

> **Current caveats (2025-09-25):** CLI binaries now build cleanly on the
> crypto suite’s Ed25519 backend (powered by `ed25519-dalek 2.2.x`), the
> transport crate re-exports the `quic` feature so
> integration harnesses must pass `--features "integration-tests quic"` when
> selecting providers, and telemetry-gated modules still emit warnings when
> optional features are disabled. Prefer the `lightweight-integration` feature
> set for memory-constrained runs and re-enable full telemetry when validating
> release candidates.

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

- The `lightweight-integration` crate feature swaps the RocksDB-backed
  `SimpleDb` for an in-memory implementation that snapshots column families to
  disk. Enable this feature when running the heaviest integration targets
  (such as `gov_dependencies` and `maybe_purge_loop`) on machines with limited
  RAM:

  ```
  cargo test -p the_block --no-default-features --features lightweight-integration --test gov_dependencies
  cargo test -p the_block --no-default-features --features lightweight-integration --test maybe_purge_loop
  ```

- To compare behaviour with the RocksDB backend, run the same targets with
  `--features storage-rocksdb` (and optionally `--no-default-features` if you
  only need the library):

  ```
  cargo test -p the_block --no-default-features --features storage-rocksdb --test gov_dependencies
  ```

- `cargo test` without additional flags continues to exercise the full RocksDB
  stack for suites that require on-disk persistence and WAL recovery.

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
