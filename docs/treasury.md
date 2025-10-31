# Treasury Executor Runbook

This runbook describes how operators deploy, monitor, and recover the treasury
executor. The executor is responsible for staging signed disbursement intents
and submitting them on-chain. The workflow is designed to tolerate multi-node
failover while preserving a single, monotonically increasing nonce watermark.

## Deployment Topology

- **Multi-node pools**: run at least two executor nodes with the
  `--treasury-executor` flag enabled. Each executor advertises a unique
  identity string via `--treasury-executor-id` and shares the same signing key
  (`--treasury-key`).
- **Lease TTL**: set `--treasury-executor-lease` to a value comfortably larger
  than the poll interval. A common deployment uses a 15s poll interval with a
  60s TTL. Shorter TTLs increase churn and should only be used in trusted lab
  environments.
- **Hot key rotation**: rotate the signing key by releasing the lease
  (`gov.treasury.executor --state <db> --json` describes the incumbent holder),
  swapping the key material, and allowing another executor to acquire the
  lease.

## Lease Watermark Semantics

Executors coordinate through a shared lease record persisted in the governance
store. The record publishes two public fields via RPC/CLI/Explorer:

- `lease_last_nonce` – the highest nonce accepted while any executor held the
  lease.
- `lease_released` – a boolean marker indicating the previous holder released
  the lease while retaining the nonce watermark.

When the lease is released the holder identity is set to `null`, the expiry is
backdated to the release timestamp, and the watermark remains unchanged.
Consumers should treat `lease_released = true` as an explicit hand-off state—
no executor currently holds the lease but future holders must start from the
watermark.

The CLI (`just gov treasury executor`) and Explorer endpoint both surface the
new watermark and release marker so operators can confirm the next executor
starts at the correct nonce.

## Failure Handling & Rollback

1. **Executor crash**: if an executor dies without releasing the lease, wait
   for the TTL to elapse. Another executor will refresh the lease and continue
   from `lease_last_nonce`.
2. **Stalled watermark**: the Grafana dashboard now includes a "Treasury
   executor nonce watermark" panel and an alert `TreasuryLeaseWatermarkLagging`.
   Investigate staging/submitter logs, then either bump the lease TTL or
   restart the affected executor.
3. **Regression**: the alert `TreasuryLeaseWatermarkRegression` pages when the
   watermark decreases (should never happen). Trigger a halt on the executor
   pool, inspect the governance store snapshot, and re-run
   `cargo test -p governance executor_failover_preserves_nonce_watermark` before
   resuming automation.
4. **Manual release**: use `gov.treasury.executor --state <db>` to inspect the
   snapshot. Call `GovStore::release_executor_lease` via the CLI when forcing a
   hand-off; the release marker is set automatically.

## Observability Checklist

- Grafana dashboards expose the new treasury panel alongside Range Boost
  telemetry (`RangeBoost forwarder failures`, `enqueue errors`, and toggle
  latency p95).
- Prometheus metrics to watch:
  - `treasury_executor_last_submitted_nonce`
  - `treasury_executor_lease_last_nonce`
  - `range_boost_forwarder_fail_total`
  - `range_boost_enqueue_error_total`
  - `range_boost_toggle_latency_seconds`
- CI now runs `just test-range-boost` with telemetry enabled and a governance
  failover smoke test to catch regressions before merge.

## Related Documentation

- [Governance overview](governance.md#treasury-executor-automation)
- [Telemetry reference](telemetry.md#treasury-executor-metrics)
- [Range Boost operations](range_boost.md)
