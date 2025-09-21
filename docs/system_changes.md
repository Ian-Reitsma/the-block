# System-Wide Economic Changes

This living document chronicles every deliberate shift in The‑Block's protocol economics and system-wide design. Each section explains the historical context, the exact changes made in code and governance, the expected impact on operators and users, and the trade-offs considered. Future hard forks, reward schedule adjustments, or paradigm pivots must append an entry here so auditors can trace how the chain evolved.

## CT Subsidy Unification (2024)

The network now mints every work-based reward directly in CT. Early devnets experimented with an auxiliary reimbursement ledger, but governance retired that approach in favour of a single, auditable subsidy store that spans storage, read delivery, and compute throughput.

### Rationale for the Switch
- **Operational Simplicity:** A unified CT ledger eliminates balance juggling, decay curves, and swap mechanics.
- **Transparent Accounting:** Subsidy flows reconcile with standard wallets, easing audits and financial reporting.
- **Predictable UX:** Users can provision gateways or upload content with a plain CT wallet—no staging balances or side ledgers.
- **Direct Slashing:** Burning CT on faults or policy violations instantly reduces circulating supply without custom settlement paths.

### Implementation Summary
- Removed the auxiliary reimbursement plumbing and its RPC surfaces, consolidating rewards into the CT subsidy store.
- Introduced global subsidy multipliers `beta`, `gamma`, `kappa`, and `lambda` for storage, read delivery, CPU, and bytes out. These values live in governance parameters and can be hot-tuned.
- Added a rent-escrow mechanism: every stored byte locks `rent_rate_ct_per_byte` CT, refunding 90 % on deletion or expiry while burning 10 % as wear-and-tear.
- Reworked coinbase generation so each block mints `STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` alongside the decaying base reward.
- Redirected the former reimbursement penalty paths to explicit CT burns, ensuring punitive actions reduce circulating supply.

Changes shipped behind feature flags with migration scripts (such as `scripts/purge_legacy_ledger.sh` and updated genesis templates) so operators could replay devnet ledgers and confirm balances and stake weights matched across the switch. Historical blocks remain valid; the new fields simply appear as zero before activation.

### Impact on Operators
- Rewards arrive entirely in liquid CT.
- Subsidy income depends on verifiable work: bytes stored, bytes served with `ReadAck`, and measured compute. Stake bonds still back service roles, and slashing burns CT directly from provider balances.
- Monitoring requires watching `subsidy_bytes_total{type}`, `subsidy_cpu_ms_total`, and rent-escrow gauges. Operators should also track `inflation.params` to observe multiplier retunes.

Archive `governance/history` to maintain a local audit trail of multiplier votes and kill-switch activations. During the first epoch after upgrade, double-check that telemetry exposes the new subsidy and rent-escrow metrics; a missing gauge usually indicates lingering legacy configuration files or dashboard panels.

### Impact on Users
- Uploads, hosting, and dynamic requests work with standard CT wallets. No staging balances or alternate instruments are required.
- Reads remain free; the cost is socialized via block-level inflation rather than per-request fees. Users only see standard rate limits if they abuse the service.

Wallet interfaces display the refundable rent deposit when uploading data and automatically return 90 % on deletion, making the lifecycle visible to non-technical users.

### Governance and Telemetry
Governance manages the subsidy dial through `inflation.params`, which exposes the five parameters:
```
 beta_storage_sub_ct
 gamma_read_sub_ct
 kappa_cpu_sub_ct
 lambda_bytes_out_sub_ct
 rent_rate_ct_per_byte
```
An accompanying emergency knob `kill_switch_subsidy_reduction` can downscale all subsidies by a voted percentage. Every retune or kill‑switch activation must append an entry to `governance/history` and emits telemetry events for on-chain tracing.

The kill switch follows a 12‑hour timelock once activated, giving operators a grace window to adjust expectations. Telemetry labels multiplier changes with `reason="retune"` or `reason="kill_switch"` so dashboards can plot long-term trends and correlate them with network incidents.

### Reward Formula Reference
The subsidy multipliers are recomputed each epoch using the canonical formula:
```
multiplier_x = (ϕ_x · I_target · S / 365) / (U_x / epoch_seconds)
```
where `S` is circulating CT supply, `I_target` is the annual inflation ceiling (currently 2 %), `ϕ_x` is the inflation share allocated to class `x`, and `U_x` is last epoch's utilization metric. Each multiplier is clamped to ±15 % of its prior value, doubling only if `U_x` was effectively zero to avoid divide-by-zero blow-ups. This dynamic retuning ensures inflation stays within bounds while rewards scale with real work.

### Pros and Cons
| Aspect | Legacy Reimbursement Ledger | Unified CT Subsidy Model |
|-------|-----------------------------|--------------------------|
| Operator payouts | Separate balance with bespoke decay | Liquid CT every block |
| UX for new users | Required staging an auxiliary balance | Wallet works immediately |
| Governance surface | Multiple mint/decay levers | Simple multiplier votes |
| Economic transparency | Harder to audit total issuance | Inflation capped ≤2 % with public multipliers |
| Regulatory posture | Additional instrument to justify | Single-token utility system with CT sub-ledgers |

### Migration Notes
Devnet operators should run `scripts/purge_legacy_ledger.sh` to wipe obsolete reimbursement data and regenerate genesis files without the legacy balance field. Faucet scripts now dispense CT. Operators must verify `inflation.params` after upgrade and ensure no deprecated configuration keys persist in configs or dashboards.

### Future Entries
Subsequent economic shifts—such as changing the rent refund ratio, altering subsidy shares, or introducing new service roles—must document their motivation, implementation, and impact in a new dated section below. This file serves as the canonical audit log for all system-wide model changes.

## Durable Compute Settlement Ledger (2025-09-21)

### Rationale for Persistence & Dual-Ledger Accounting

- **Crash resilience:** The in-memory compute settlement shim dropped balances on restart. Persisting CT flows (with legacy industrial columns retained for tooling) in RocksDB guarantees recovery, even if the node or process exits unexpectedly.
- **Anchored accountability:** Governance required an auditable trail that explorers, operators, and regulators can replay. Recording sequences, timestamps, and anchors ensures receipts reconcile with the global ledger.
- **Ledger clarity:** Providers and buyers need to understand CT balances after every job. Persisting the ledger avoids race conditions when reconstructing balances from mempool traces and keeps the legacy industrial column available for regression tooling.

### Implementation Summary

- `Settlement::init` now opens (or creates) `compute_settlement.db` inside the configured settlement directory, wiring sled-style helpers that load or default each sub-tree (`ledger_ct`, `ledger_it`, `metadata`, `audit_log`, `recent_roots`, `next_seq`). Test builds without `storage-rocksdb` transparently fall back to an ephemeral directory.
- Every accrual, refund, or penalty updates both the in-memory ledger and the persisted state via `persist_all`, bumping a monotonic sequence and recomputing the Merkle root (`compute_root`).
- `Settlement::shutdown` always calls `persist_all` on the active state and flushes RocksDB handles before dropping them, ensuring integration harnesses (and crash recovery drills) see fully durable CT balances (with zeroed industrial fields) even if the node exits between accruals.
- `Settlement::submit_anchor` hashes submitted receipts, records the anchor in `metadata.last_anchor_hex`, pushes a marker into the audit deque, and appends a JSON line to the on-disk audit log through `state::append_audit`.
- Activation metadata (`metadata.armed_requested_height`, `metadata.armed_delay`, `metadata.last_cancel_reason`) captures the reason for every transition between `DryRun`, `Armed`, and `Real` modes. `Settlement::arm`, `cancel_arm`, and `back_to_dry_run` persist these fields immediately and emit telemetry via `SETTLE_MODE_CHANGE_TOTAL{state}`.
- Telemetry counters `SETTLE_APPLIED_TOTAL`, `SETTLE_FAILED_TOTAL{reason}`, `SLASHING_BURN_CT_TOTAL`, and `COMPUTE_SLA_VIOLATIONS_TOTAL{provider}` expose a live view of accruals, refunds, and penalties. Dashboards can alert on stalled anchors or repeated SLA violations.
- RPC endpoints `compute_market.provider_balances`, `compute_market.audit`, and `compute_market.recent_roots` serialize the persisted data so the CLI and explorer can render provider balances, audit trails, and continuity proofs. Integration coverage lives in `node/tests/compute_settlement.rs`, `cli/tests/compute.rs`, and `explorer/tests/compute_settlement.rs`.

### Operational Impact

- **Operators** should monitor the new RPCs and Prometheus counters to ensure balances drift as expected, anchors land on schedule, and SLA burns are visible. Automate backups of `compute_settlement.db` alongside other state directories.
- **Explorers and auditors** can subscribe to the audit feed, correlate sequence numbers with Merkle roots, and flag any divergence between local mirrors and the node-provided anchors.
- **Governance and finance** teams gain deterministic evidence of CT burns, refunds, and payouts, unblocking treasury reconciliation and upcoming SLA enforcement proposals.

### Migration Notes

- Nodes upgrading from the in-memory shim should point `settlement_dir` (or the default data directory) at persistent storage before enabling `Real` mode. The first startup migrates balances into RocksDB with a zeroed sequence.
- Automation that previously scraped in-process metrics must switch to the RPC surfaces described above. CLI invocations now require the `sqlite-storage` feature (or the `full` bundle) to display the persisted audit snapshots.
- Backups should include `compute_settlement.db` and the `audit.log` file written by `state::append_audit` so post-incident reviews retain both ledger state and anchor evidence.
