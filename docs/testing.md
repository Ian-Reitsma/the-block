# Integration and Chaos Testing

This document outlines the strategy for integration and chaos testing in **The-Block**.

## Network Integration

- `tests/net_integration.rs` exercises a multi-node simulation.
- Partitions are modelled by stepping nodes independently and rejoining to verify
  deterministic fork choice.

## Governance Upgrades

- `tests/gov_upgrade.rs` exports a governance snapshot and ensures the artifact
  contains expected fields. This verifies that upgrade templates are stable.

## Storage Chaos

- `tests/storage_chaos.rs` randomly deletes RocksDB WAL files after writes and
  confirms the database can recover without data loss.

## Concurrent Market Stress

- `tests/concurrent_markets.rs` runs DEX swaps and compute market price band
  calculations on separate threads to surface race conditions.

## Long-Running Test Harness

- `scripts/longtest.sh` repeatedly runs the full test suite. Pass a custom
  duration in seconds to control the run length (12 hours by default).

## Deterministic Seeds

- The simulation harness uses the `TB_SIM_SEED` environment variable to seed its
  RNG, enabling reproducible scenarios across the test suite.

## RPC Fault Injection

- `node/src/rpc/client.rs` accepts `TB_RPC_FAULT_RATE` to randomly inject
  request failures, exercising client retry logic under adverse conditions.

## Coverage Reporting

- `.github/workflows/coverage.yml` runs Tarpaulin in CI and publishes an XML
  coverage report as an artifact.

## Running Tests

```
cargo test --all --features test-telemetry --release
```
