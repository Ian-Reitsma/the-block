# Governance Parameters

The chain exposes a handful of live‑tunable parameters that can be updated via on‑chain governance. Each proposal carries a key, a new value, and bounds. When a proposal passes, the change is queued and activates at the next epoch boundary.

## Keys

| Key | Default | Min | Max | Unit |
| --- | --- | --- | --- | --- |
| `snapshot_interval_secs` | 30 | 5 | 600 | seconds |
| `consumer_fee_comfort_p90_microunits` | 2500 | 500 | 25 000 | microunits |
| `industrial_admission_min_capacity` | 10 | 1 | 10 000 | microshards/sec |

## Proposing a Change

Use `blockctl` to submit and vote on proposals:

```bash
# Raise the comfort threshold to 3500 microunits
blockctl gov propose --key consumer_p90_comfort \
  --new-value 3500 --min 500 --max 25000 --reason "scale industrial"
```

After submitting, cast votes and wait for the proposal to pass. The metrics exporter exposes `param_change_pending{key}` set to `1` while a change is awaiting activation.

## Activation Timeline

1. **Vote phase:** proposal is open for votes until the configured deadline.
2. **Pending:** once it passes, `param_change_pending{key}=1` until the activation epoch.
3. **Activation:** at the epoch boundary, the runtime applies the new value and `param_change_active{key}` reflects it. The pending gauge returns to `0` and a log line `gov_param_activated` is emitted.

Changes apply atomically at epoch boundaries; mid‑epoch behaviour is unaffected.

## Promoting Seeds

To revert a change within the rollback window, use `blockctl gov rollback` which restores the previous value and updates `param_change_active{key}` accordingly.

For a deeper overview of governance mechanics, see `docs/governance.md`.
