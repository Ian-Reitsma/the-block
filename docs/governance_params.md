# Governance Parameters
> **Review (2025-09-25):** Synced Governance Parameters guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The chain exposes a handful of live‑tunable parameters that can be updated via on‑chain governance. Each proposal carries a key, a new value, and bounds. When a proposal passes, the change is queued and activates at the next epoch boundary.

## Keys

| Key | Default | Min | Max | Unit |
| --- | --- | --- | --- | --- |
| `snapshot_interval_secs` | 30 | 5 | 600 | seconds |
| `consumer_fee_comfort_p90_microunits` | 2500 | 500 | 25 000 | microunits |
| `industrial_admission_min_capacity` | 10 | 1 | 10 000 | microshards/sec |
| `mempool.fee_floor_window` | 256 | 1 | 4096 | samples |
| `mempool.fee_floor_percentile` | 75 | 0 | 100 | percent |
| `bridge_min_bond` | 50 | 0 | 1 000 000 | tokens |
| `bridge_duty_reward` | 5 | 0 | 1 000 000 | tokens |
| `bridge_failure_slash` | 10 | 0 | 1 000 000 | tokens |
| `bridge_challenge_slash` | 25 | 0 | 1 000 000 | tokens |
| `bridge_duty_window_secs` | 300 | 1 | 86 400 | seconds |
| `read_subsidy_viewer_percent` | 40 | 0 | 100 | percent |
| `read_subsidy_host_percent` | 30 | 0 | 100 | percent |
| `read_subsidy_hardware_percent` | 15 | 0 | 100 | percent |
| `read_subsidy_verifier_percent` | 10 | 0 | 100 | percent |
| `read_subsidy_liquidity_percent` | 5 | 0 | 100 | percent |

## Proposing a Change

Use the `contract` CLI to queue parameter updates for the supported keys:

```bash
# Expand the fee-floor window to cover the last 512 fees
contract gov param update mempool.fee_floor_window 512 --state gov.db

# Nudge the fee-floor percentile to 70%
contract gov param update mempool.fee_floor_percentile 70
```

After submitting, cast votes and wait for the proposal to pass. The metrics exporter exposes `param_change_pending{key}` set to `1` while a change is awaiting activation. Fee-floor updates append JSON records under `governance/history/fee_floor_policy.json`, increment `fee_floor_window_changed_total`, and appear via the explorer endpoint `/mempool/fee_floor_policy` so operators can audit historical values. Other parameters can still be tuned through JSON proposals even if the CLI helper does not expose them yet.

Bridge incentive keys update the live duty and accounting ledger. When a proposal adjusts `bridge_min_bond`, `bridge_duty_reward`, `bridge_failure_slash`, `bridge_challenge_slash`, or `bridge_duty_window_secs`, the runtime immediately pushes the refreshed values into the global incentive snapshot; subsequent deposits and withdrawals enforce the new thresholds without requiring a restart. Operators can confirm the active values through `bridge.relayer_status`, `bridge.relayer_accounting`, or `blockctl bridge accounting`.

Read subsidy split keys control how the node credits viewer, host, hardware, verifier, and liquidity accounts when a block finalizes. Updating any of the `read_subsidy_*_percent` values retunes the live distribution map immediately—the acknowledgement worker and mining logic read from the updated percentages on the next receipt. Liquidity receives any unused share automatically, so governance can safely lower individual buckets without stranding CT.

## Activation Timeline

1. **Vote phase:** proposal is open for votes until the configured deadline.
2. **Pending:** once it passes, `param_change_pending{key}=1` until the activation epoch.
3. **Activation:** at the epoch boundary, the runtime applies the new value and `param_change_active{key}` reflects it. The pending gauge returns to `0` and a log line `gov_param_activated` is emitted.

Changes apply atomically at epoch boundaries; mid‑epoch behaviour is unaffected.

## Promoting Seeds

To revert a change within the rollback window, use `contract gov rollback <key>` which restores the previous value, updates `param_change_active{key}`, and records another history entry for traceability.

For a deeper overview of governance mechanics, see `docs/governance.md`.
