# Telemetry overview
> **Review (2025-09-25):** Synced Telemetry overview guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

See [docs/economics.md](../economics.md#epoch-retuning-formula) for subsidy formulas and ROI guidance. Historical context on the subsidy transition lives in [docs/system_changes.md](../system_changes.md#ct-subsidy-unification-2024).

Headline panels show:
- **Probe success rate 10m** – expect ~100%.
- **Convergence p95 (s)** – normal < 3s.
- **Consumer fee p90 vs comfort** – track for fee spikes.
- **Industrial defer ratio 10m** – high values indicate capacity pressure.
- **SLA misses** – monitor `industrial_rejected_total{reason="SLA"}` for deadline violations.
- **Settlement pipeline** – correlate `SETTLE_APPLIED_TOTAL`, `SETTLE_FAILED_TOTAL{reason}`, and `SETTLE_MODE_CHANGE_TOTAL{state}` with the RPCs `compute_market.provider_balances`/`compute_market.recent_roots` to ensure ledger events flush to disk before and after upgrades.
- **QUIC provider mix** – graph `quic_provider_connect_total{provider}` alongside `quic_handshake_fail_total{peer,provider}` to spot regressions when rolling Quinn or s2n updates.
- **SLA slashing** – monitor `SLASHING_BURN_CT_TOTAL` and `COMPUTE_SLA_VIOLATIONS_TOTAL{provider}` to catch runaway penalty streaks or failed enforcement.
- **Storage provider RTT/loss** – track `storage_provider_rtt_ms` and `storage_provider_loss_rate`.
- **Read denials & issuance** – watch `read_denied_total{reason}` and `subsidy_bytes_total{type="read"}`; rent escrow via `rent_escrow_locked_ct_total`, `rent_escrow_refunded_ct_total`, and `rent_escrow_burned_ct_total`. Sudden `subsidy_auto_reduced_total` or `kill_switch_trigger_total` increments indicate global inflation dampening events and should be cross-referenced with `governance/history`.

Each panel exposes drill-down links to the underlying metric definition in
`monitoring/metrics.json`. The generated dashboard surfaces the current value and
the raw metric name so operators can pivot into the CLI or automation. Set alert
thresholds at roughly 2× the 30‑day moving average to catch regressions without
paging on normal bursts.

To poll metrics remotely, point the dashboard helpers or a custom script at the
node’s `/metrics` exporter (for example `http://node-host:9898/metrics`). The
`TELEMETRY_ENDPOINT` environment variable controls the target for both the
Docker compose recipe and `scripts/monitor_native.sh`.

Use `scripts/telemetry_sweep.sh` to generate a static `status/index.html` snapshot.

The sweep script captures subsidy multipliers (`beta`, `gamma`, `kappa`,
`lambda`) and rent-rate values in its HTML header, providing a point-in-time
record that auditors can compare against `governance/history` entries during
post-mortems.

Operators can query subsidy settings and bonded stakes directly via JSON-RPC:

- `inflation.params` exposes the live `beta/gamma/kappa/lambda` multipliers and current `rent_rate_ct_per_byte`.
- `stake.role` returns the CT bonded for each service role (gateway, storage, exec) under a given account.

Including these calls in periodic telemetry sweeps helps correlate dashboard
metrics with on-chain governance parameters and stake distribution.
