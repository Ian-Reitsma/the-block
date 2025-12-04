# Economics and Governance

Everything settles in CT. Consumer workloads, industrial compute/storage, and governance treasury actions all share the same ledger so explorers/CLI/telemetry never disagree.

## CT Supply and Sub-Ledgers
- Coinbases embed `STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` fields (see `node/src/blockchain/block_binary.rs`). Each bucket mints CT but is accounted separately for policy analysis.
- Industrial workload gauges (`industrial_backlog`, `industrial_utilization`) flow from storage/compute telemetry into `Block::industrial_subsidies()`.
- Personal rebates are ledger entries only. They auto-apply to the submitter’s own write traffic before dipping into transferable CT and never circulate.

## Energy Market Economics
- **Single-token model** — Energy payouts settle in CT just like storage/compute. Credits (`EnergyCredit`) and receipts (`EnergyReceipt`) are internal ledger objects stored in `SimpleDb::open_named(names::ENERGY_MARKET, …)`; settlement burns meter credits, decrements provider capacity, and records `EnergyReceipt { buyer, seller, kwh_delivered, price_paid, treasury_fee, slash_applied }`.
- **Treasury integration** — `node::energy::settle_energy_delivery` forwards `treasury_fee + slash_applied` to `NODE_GOV_STORE.record_treasury_accrual`, so explorer/CLI treasury views capture energy fees without extra plumbing. Governance proposals can earmark these accruals like any other treasury inflow.
- **Governance parameters** — `energy_min_stake`, `energy_oracle_timeout_blocks`, and `energy_slashing_rate_bps` live in the shared `governance` crate (`ParamKey::EnergyMinStake`, etc.). Proposals use the same `ParamSpec` flow as other knobs; once activated, `node::energy::set_governance_params` updates the runtime config and snapshots the energy sled DB. Outstanding work adds new payloads (batch vs real-time settlement, dependency graph validation) tracked in `docs/architecture.md#energy-governance-and-rpc-next-tasks`.
- **Oracle economics** — Meter readings produce `EnergyCredit` entries keyed by the reading hash (BLAKE3 over provider, meter, readings, timestamp, signature). Credits expire after `energy_oracle_timeout_blocks`; stale readings cannot be settled and must be re-issued. `energy.submit_reading` RPC will soon enforce signature validation and multi-reading attestations, with slashing telemetry + dispute RPCs covering bad actors.
- **CLI/RPC visibility** — `tb-cli energy market --verbose` and `energy.market_state` expose provider capacity, price, stake, outstanding credits, and receipts so explorers can mirror the same tables. Upcoming explorer work adds energy provider tables, receipt timelines, and slash summaries (see `AGENTS.md` tasks).
- **Dispute flow** — Until dedicated dispute RPCs land, governance proposals (e.g., temporarily raising `energy_slashing_rate_bps` for a provider, pausing settlement) act as the economic kill switch. Once the dispute endpoints ship they will create ledger anchors referencing disputed meter hashes while preserving CT accounting invariants.

## Multipliers and Emissions
- Per-epoch utilisation `U_x` feeds the “one dial” multiplier:
  \[
  \text{multiplier}_x = \frac{\phi_x I_{\text{target}} S / 365}{U_x / \text{epoch\_secs}}
  \]
  Adjustments clamp to ±15 % to prevent thrash. Near-zero utilisation doubles the multiplier to keep incentives alive; governance can override via `kill_switch_subsidy_reduction`.
- Miner base reward follows the logistic curve implemented in `node/src/consensus/leader.rs`:
  \[
  R_0(N) = \frac{R_{\max}}{1+e^{\xi (N-N^\star)}}
  \]
  with hysteresis (ΔN ≈ √N*) that damps flash joins/leaves.
- Governance, ledger, CLI, explorer, and metrics aggregator all pull multiplier history through the shared `governance` crate to avoid drift.

## Fee Lanes and Rebates
- `node/src/fee` defines the lane taxonomy (consumer, industrial, priority, treasury). `node/src/fees` implements QoS eviction and rebate books shared with RPC.
- Lane-aware mempool enforcement sits in `node/src/mempool` (see `docs/architecture.md#fee-lanes-and-rebates`). Each block nudges the base fee toward a fullness target while telemetry exposes `mempool_fee_floor_*` gauges.
- Rebates are persisted ledger entries exposed via RPC (`node/src/rpc/fees.rs`) and CLI (`cli/src/fee_estimator.rs`).

## Service Badges and Citizenship
- Operators earn service badges when uptime/latency stay within governance thresholds. `node/src/service_badge.rs` calculates eligibility; telemetry publishes `BADGE_ISSUED_TOTAL`, `COMPUTE_PROVIDER_UPTIME`, etc.
- Badges gate governance votes (Operators + Builders houses) and feed range-boost multipliers plus ANN mesh prioritisation.

## Treasury and Disbursements
- Governance proposals now carry explicit treasury-disbursement payloads in addition to param updates. Each disbursement advances through the canonical state machine: **draft → voting → queued → timelocked → executed → finalized/rolled-back**. Drafts are local JSON payloads (stored under `examples/governance/`) validated with `foundation_serialization` schemas before the proposer signs and submits. Voting/timelock rules piggyback on the bicameral governance machinery (see `governance/src/bicameral.rs`), so disbursements inherit quorum, snapshot, and activation semantics.
- Once a disbursement proposal passes, `GovStore` persists the queued entry in sled and snapshots the activation epoch + prior rollbacks to `provenance.json` using first-party encoding (Option A from the task brief). The rollback window remains **block-height bounded** via `governance::store::ROLLBACK_WINDOW_EPOCHS`, guaranteeing deterministic replay on both x86_64 and AArch64.
- Executions emit CT receipts inside the consolidated ledger—no new token types—and every transition (queued, timelocked, executed, rollback) records a ledger journal entry so the explorer and CLI timelines never diverge. Rollbacks simply mark the disbursement as `RolledBack { rolled_back_at, reason }` and append a compensating ledger entry; finalized executions capture the `tx_hash`, execution height, and attested receipt bundle.
- Metrics wiring tracks both balances and pipeline health: `treasury_balance_ct`, `treasury_disbursement_backlog`, and `governance_disbursements_total{status}`. The metrics aggregator exposes `/treasury/summary` and `/governance/disbursements` so dashboards can chart backlog age, quorum wait time, and execution throughput alongside existing treasury gauges. Explorer timelines render the same data (proposal metadata, vote outcomes, timelock window, execution tx, affected accounts, receipts, and rollback annotations).

## Proposal Lifecycle
1. Snapshot of eligible voters occurs on proposal creation (bicameral: Operators + Builders).
2. Secret ballots + timelocks enforced by `governance/src/bicameral.rs`.
3. Parameter changes apply next epoch; upgrades require supermajority plus rollback windows.
4. Emergency catalog/app-layer overrides auto-expire and must be fully logged.

## Governance Parameters
- `governance/src/params.rs` exposes typed knobs for fee floors, multipliers, SLA slashing, telemetry sampling, mesh toggles, AI diagnostics, etc.
- Every integration (node, CLI, explorer, metrics aggregator) uses the same crate so policy proofs line up with on-chain values.
- Historical policy snapshots stream through RPC + CLI; explorers visualise the same baseline.

## Commit–Reveal and PQ Hooks
- `node/src/commit_reveal.rs` implements Dilithium-based commits when compiled with `pq-crypto`, otherwise BLAKE3 commitments. Used for ballots, treasury releases, and challenge proofs.
- Governance DAG nodes store both commit and reveal payloads plus telemetry for mismatches.

## Treasury Kill Switch and Risk Controls
- `governance/src/state.rs` wires `kill_switch_subsidy_reduction`, `kill_switch_fee_floor`, and range-boost toggles to treasury guardians.
- Risk mitigations from the former `docs/risk_register.md`, `docs/audit_handbook.md`, and `docs/system_changes.md` live here plus `docs/security_and_privacy.md`.

## Settlement and Audit Guarantees
- `tools/settlement_audit` and `node/tests/settlement_audit.rs` reconcile receipts against ledger anchors. Operators must keep `cargo test -p the_block --test settlement_audit --release` green.
- Settlement switch semantics (industrial vs consumer routing) live in `node/src/compute_market/settlement` and `node/src/storage/pipeline`. Governance toggles them via params documented here.

## Governance Tooling
- CLI: `cli/src/gov.rs` now exposes the disbursement workflow end-to-end: `tb-cli gov disburse create|preview|submit|show|queue|execute|rollback` plus `--schema`/`--check` helpers for JSON payload validation. Existing proposal/DAG helpers remain alongside `cli/src/service_badge.rs` (badge status) and `cli/src/telemetry.rs` (wrapper metadata).
- Explorer + log indexer share the same governance crate via `foundation_serialization` + `foundation_sqlite` wrappers.
- Metrics aggregator publishes `/governance`, `/treasury`, `/wrappers`, and `/bridge` dashboards plus webhook outputs (`docs/operations.md#metrics-aggregator`).

## Ledger Invariants
- Ledger invariants from the former `docs/ledger_invariants.md` now anchor here: no mint-to-EOA, subsidy buckets sum to the recorded total, governance history is monotonic, badge revocations are fully logged, and macro-block anchors must match the gossip replay harness.
