# Compute Settlement Modes
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The compute marketplace supports a two-phase switch from dry-run to real
settlement on devnet. Operators can arm the system to start applying real
debits and CT payouts after a delay, cancel before activation, or revert to
dry-run on demand.

## Modes

- `DryRun` – receipts are emitted but no funds move.
- `Armed` – scheduled to flip to `Real` at a specific block height.
- `Real` – debits buyers and pays providers in CT for each receipt.

## RPC Controls

```
compute_arm_real{ activate_in_blocks: N }
compute_cancel_arm()
compute_back_to_dry_run{ reason }
```

## Telemetry

- `settle_applied_total`
- `settle_failed_total{reason}`
- `settle_mode_change_total{to}`