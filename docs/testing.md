# Integration and Chaos Testing

This document outlines the strategy for integration and chaos testing in **The-Block**.

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

## RPC Fault Injection

- `node/src/rpc/client.rs` accepts `TB_RPC_FAULT_RATE` to randomly inject
  request failures, and `node/tests/rpc_fault_injection.rs` verifies that the
  client surfaces these errors for resiliency testing.

## Coverage Reporting

- `.github/workflows/coverage.yml` runs Tarpaulin in CI and publishes an XML
  coverage report as an artifact.

## Running Tests

```
cargo test --all --features test-telemetry --release
```
