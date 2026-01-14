# Economics and Governance

> **Plain-Language Overview**
>
> **BLOCK is the single token.** Everything in The Block settles in BLOCK — payments, rewards, fees, treasury disbursements. There's no second currency. Some internal labels (for example `coin_c`/`coin_i`) remain for compatibility, but they are BLOCK-denominated buckets, not separate tokens.
>
> **How BLOCK moves around:**
> | Flow | What Happens |
> |------|--------------|
> | **Mining** | New BLOCK is minted in each block's "coinbase" (the first transaction) |
> | **Subsidies** | Part of that coinbase goes to storage/compute/bandwidth providers (`STORAGE_SUB`, `READ_SUB`, `COMPUTE_SUB`) |
> | **Fees** | Users pay BLOCK for transactions; fees are split between validators and treasury |
> | **Rebates** | Users may receive "rebates" — ledger entries that reduce future costs (not separate tokens) |
> | **Treasury** | Community fund; disbursements require governance votes |
> 
> **Governance in a nutshell:** BLOCK holders vote on proposals. Proposals can change parameters (like fee floors), allocate treasury funds, or upgrade the network. There's a timelock between approval and activation to allow for rollbacks if something goes wrong.

Everything settles in BLOCK. Consumer workloads, industrial compute/storage, and governance treasury actions all share the same ledger so explorers/CLI/telemetry never disagree.

## BLOCK Supply and Sub-Ledgers
- Coinbases embed `STORAGE_SUB`, `READ_SUB`, and `COMPUTE_SUB` fields (see `node/src/blockchain/block_binary.rs`). Each bucket mints BLOCK but is accounted separately for policy analysis.
- `coin_c`/`coin_i` are legacy ledger labels for BLOCK-denominated buckets; they do not represent a second token. Lane routing happens at transaction submission, while subsidy accounting stays unified in BLOCK.
- Industrial workload gauges (`industrial_backlog`, `industrial_utilization`) feed the subsidy allocator (`node/src/economics/subsidy_allocator.rs`) and replay-derived market metrics (`node/src/economics/replay.rs`).
- Personal rebates are ledger entries only. They auto-apply to the submitter’s own write traffic before dipping into transferable BLOCK and never circulate.

## Network-Driven BLOCK Issuance

> **Plain English:** Instead of targeting a fixed inflation rate, the protocol mints new BLOCK based on how busy (and how decentralized) the network actually is, while respecting the 40 M cap.

The canonical issuance controller lives in `node/src/economics/network_issuance.rs` and is mirrored in telemetry/CLI/explorer. Every block reward is derived from the same four-factor formula:

\[
\text{reward} = \text{base} \times \text{activity} \times \text{decentralization} \times \text{supply\_decay}
\]

- **Base reward** — distribute 90 % of the 40 M cap evenly across the expected number of blocks (`max_supply_block`, `expected_total_blocks`). Using 90 % leaves room for tail emission.
- **Activity multiplier** — geometric mean of transaction-count ratio, transaction-volume ratio, and `(1 + avg_market_utilization)`; each input is smoothed via adaptive baselines (EMA with governance-set clamps) so a growing network naturally earns more while a quiet network decays back toward 1.0 ×. Bounds: `[activity_multiplier_min, activity_multiplier_max]`.
- **Decentralization factor** — `sqrt(unique_miners / baseline_miners)` with the same EMA/bounds treatment. More independent miners increase rewards; a shrinking set dampens them. Bounds: `[decentralization_multiplier_min, decentralization_multiplier_max]`.
- **Supply decay** — linear decay based on remaining supply `(MAX_SUPPLY_BLOCK - emission) / MAX_SUPPLY_BLOCK`; prevents the cap from being exceeded and emulates a halving-style tail.

All state for this controller (EMA baselines, clamp bounds, alpha values) is stored in the governance params struct `NetworkIssuanceParams` and exposed via telemetry so replay stays deterministic.

### Telemetry-driven gating

Every epoch `node/src/lib.rs` increments the on-chain counters `economics_epoch_tx_count`,
`economics_epoch_tx_volume_block`, and `economics_epoch_treasury_inflow_block` as transactions hit the
chain. Those counters, along with `recent_miners` and the stored circulations, feed `NetworkIssuanceController`
inside `execute_epoch_economics()`, which writes the next `economics_block_reward_per_block`.
Telemetry mirrors the same values (`ECONOMICS_EPOCH_*` gauges plus `ECONOMICS_BLOCK_REWARD_PER_BLOCK`)
so Launch Governor's autopilot can verify throughput, volume, and treasury inflow before flipping the
testnet → mainnet gate. Each node persists the latest base reward in `ChainDisk` so restarts follow the same
control decisions and progress toward the live network with no manual tuning.

### Legacy Inflation Controller (compatibility only)

The pre-BLOCK codebase exposed knobs such as `inflation_target_bps`, `inflation_controller_gain`, `min_annual_issuance_block`, and `max_annual_issuance_block`. These still exist for backward compatibility with tooling, but they are no longer the primary monetary policy. Any proposal that touches those fields must explicitly justify how it keeps the network-driven issuance formula aligned; otherwise the docs and `NetworkIssuanceController` are treated as the source of truth.

## Energy Market Economics
- **Single-token model** — Energy payouts settle in BLOCK just like storage/compute. Credits (`EnergyCredit`) and receipts (`EnergyReceipt`) are internal ledger objects stored in `SimpleDb::open_named(names::ENERGY_MARKET, …)`; settlement burns meter credits, decrements provider capacity, and records `EnergyReceipt { buyer, seller, kwh_delivered, price_paid, treasury_fee, slash_applied }`.
- **Treasury integration** — `node::energy::settle_energy_delivery` forwards `treasury_fee + slash_applied` to `NODE_GOV_STORE.record_treasury_accrual`, so explorer/CLI treasury views capture energy fees without extra plumbing. Governance proposals can earmark these accruals like any other treasury inflow.
- **Governance parameters** — `energy_min_stake`, `energy_oracle_timeout_blocks`, and `energy_slashing_rate_bps` live in the shared `governance` crate (`ParamKey::EnergyMinStake`, etc.). Proposals use the same `ParamSpec` flow as other knobs; once activated, `node::energy::set_governance_params` updates the runtime config and snapshots the energy sled DB. Outstanding work adds new payloads (batch vs real-time settlement, dependency graph validation) tracked in `docs/architecture.md#energy-governance-and-rpc-next-tasks`.
- **Oracle economics** — Meter readings produce `EnergyCredit` entries keyed by the reading hash (BLAKE3 over provider, meter, readings, timestamp, signature). Credits expire after `energy_oracle_timeout_blocks`; stale readings cannot be settled and must be re-issued. `energy.submit_reading` RPC will soon enforce signature validation and multi-reading attestations, with slashing telemetry + dispute RPCs covering bad actors.
- **CLI/RPC visibility** — `contract-cli energy market --verbose` and `energy.market_state` expose provider capacity, price, stake, outstanding credits, and receipts so explorers can mirror the same tables. Upcoming explorer work adds energy provider tables, receipt timelines, and slash summaries (see `AGENTS.md` tasks).
- **Dispute flow** — Until dedicated dispute RPCs land, governance proposals (e.g., temporarily raising `energy_slashing_rate_bps` for a provider, pausing settlement) act as the economic kill switch. Once the dispute endpoints ship they will create ledger anchors referencing disputed meter hashes while preserving BLOCK accounting invariants.

## Ad Market Pricing and Claims

> **Plain English:** Ads are priced from two signals at once: *creative quality* (how likely the ad converts) and *cohort quality* (how reliable/fresh/private the audience is). Cohort quality is penalized when readiness is unstable, presence proofs are stale, or privacy budgets are exhausted.

### Quality-Adjusted Pricing

Let `B` be the creative base bid (USD micros). Creative quality `Q_creative` is derived from action rate + lift (see `MarketplaceConfig.quality_*`). Cohort quality `Q_cohort` uses freshness, privacy, and readiness:

```
F = (w1 * under_1h_ppm + w2 * hours_1_to_6_ppm +
     w3 * hours_6_to_24_ppm + w4 * over_24h_ppm) / 1_000_000
R = clamp(ready_streak_windows / readiness_target_windows, readiness_floor, 1.0)
P = clamp(min(privacy_remaining_ppm, 1_000_000 - privacy_denied_ppm) / 1_000_000,
          privacy_floor, 1.0)
Q_cohort = clamp((F * P * R)^(1/3), cohort_quality_floor, cohort_quality_ceiling)
effective_bid = B * Q_creative * Q_cohort
```

Defaults (unless overridden in governance/config):
- Freshness weights: `w1=1_000_000`, `w2=800_000`, `w3=500_000`, `w4=200_000`.
- Floors: `readiness_floor=0.10`, `privacy_floor=0.10`, `cohort_quality_floor=0.10`.
- Ceiling: `cohort_quality_ceiling=2.50`.
- Readiness target: `readiness_target_windows=6`.

Telemetry exports `ad_quality_multiplier_ppm{component}` along with readiness/freshness/privacy gauges so operators can audit which component drove a discount/premium.

### Resource-Cost Coupling

Ad resource floors blend shared resource signals with live scarcity:

- Bandwidth cost uses storage rent signals (rent-per-byte converted to USD micros) plus the rolling storage median.
- Verification cost uses the compute-market spot price per unit (industrial lane) converted via the token oracle, multiplied by `resource_floor.verifier_compute_units_per_proof` and clamped by the legacy verifier median when the spot signal is missing.
- Host cost continues to use rolling medians, but the cost basis is recomputed each reservation so compute scarcity propagates into ad floors.
- A utilization-sensitive scarcity multiplier (`[0.8, 2.0]`) scales the floor using both cost basis and PI-controller observed vs target utilization.

The scaled breakdown is persisted in receipts to keep replays deterministic, and telemetry exposes `ad_compute_unit_price_usd_micros`, `ad_cost_basis_usd_micros{component}`, and clearing prices alongside medians.

### Claims Registry + Attribution

Ad payouts route through a claims registry keyed by `domain` + `role` with optional app/DID anchors. Claims bind payout addresses per role (publisher/host/hardware/verifier/liquidity/viewer) and are registered via `ad_market.register_claim_route`; they persist in marketplace metadata and flow into settlement breakdowns/receipts for explorer/CLI attribution. If no claim exists, the default role splits remain but have no address hints.

Conversions can optionally include device-link attestations (explicit opt-in) to improve dedup/attribution without elevating cohorts to on-chain objects. ROI summaries are exposed via `ad_market.attribution` and combine selector spend, conversion value, and uplift snapshots.

## Multipliers and Emissions

> **Plain English:** The network automatically adjusts how much BLOCK goes to different services based on usage. If storage usage is low, storage rewards increase to attract providers. If usage is high, rewards dampen to avoid overpaying.
>
> **Symbol guide:**
> | Symbol | Meaning |
> |--------|---------|
> | `phi_x` | Policy knob for this service (set by governance) |
> | `I_target` | Target BLOCK issuance per year |
> | `S` | Share allocated to this service type |
> | `U_x` | Real usage this epoch |
> | `epoch_secs` | How long an epoch lasts |

- Per-epoch utilisation `U_x` feeds the "one dial" multiplier:
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

> **Implementation note:** The subsidy allocator, multiplier controller, and ad/tariff drift controllers run every epoch today. Provider margin inputs for storage/compute/ad are still being wired (see `AGENTS.md §15`), so telemetry may show placeholders until those metrics land. The formulas above remain authoritative and must be kept in sync with `node/src/economics/**/*.rs`.

## Fee Lanes and Rebates

> **Plain English:** Fee lanes are like different queues at the post office. Each lane has its own rules and pricing.
>
> | Lane | Who Uses It | How Pricing Works |
> |------|-------------|-------------------|
> | **Consumer** | Regular wallet users | Base fee + tip; auto-adjusts based on mempool fullness |
> | **Industrial** | Storage/compute providers | Higher base, but subsidized by block rewards |
> | **Priority** | Anyone who needs fast inclusion | Pay more, get included sooner |
> | **Treasury** | Governance disbursements | Fixed rates set by governance |
>
> **Rebates** are ledger entries that reduce your future costs. If you overpaid or qualify for a promotion, you get a rebate that auto-applies to your next transactions. Rebates are NOT tokens — you can't send them to someone else.

- `node/src/fee` defines the lane taxonomy (consumer, industrial, priority, treasury). `node/src/fees` implements QoS eviction and rebate books shared with RPC.
- Lane-aware mempool enforcement sits in `node/src/mempool` (see `docs/architecture.md#fee-lanes-and-rebates`). Each block nudges the base fee toward a fullness target while telemetry exposes `fee_floor_current` plus per‑lane `fee_floor_warning_total` / `fee_floor_override_total`.
- Rebates are persisted ledger entries exposed via RPC (`node/src/rpc/fees.rs`) and CLI (`cli/src/fee_estimator.rs`).

## Lane Congestion Pricing

> **Plain English:** The lane pricing engine treats each lane as its own queue with feedback control. Pricing rises smoothly as utilisation approaches 100 %, and a PI controller dampens oscillations so users see predictable fees.

- **Queueing model (M/M/1):** Utilisation `ρ = λ / μ` per lane, clamped to `(0, 1)`. Congestion multiplier:
  \[
  C(ρ) = 1 + k \left(\frac{ρ}{1-ρ}\right)^n
  \]
  where `k` and `n` are lane-specific sensitivity parameters.
- **PI controllers:** Per-lane proportional–integral loops hold utilisation near targets:
  \[
  A_{t+1} = A_t \cdot \left(1 + K_p e_t + K_i \sum e_t\right),\quad e_t = ρ_{\text{target}} - ρ_{\text{actual}}
  \]
  with anti-windup caps to prevent runaway adjustments.
- **Market demand multiplier (industrial):** Market signals feed a bounded multiplier:
  \[
  M(D) = 1 + α \cdot \frac{e^{βD}-1}{e^{β}-1},\quad D \in [0, 1]
  \]
  giving smooth growth from `1.0` to `1.0 + α`.
- **Cross-lane arbitrage prevention:** Industrial fees floor against consumer fees with a premium `δ`, and congestion signals are lane-local so bursts in one lane do not drag the other.
- **Composite fees:**
  - Consumer: `F_c = B_c · C_c(ρ_c) · A_c(t)`
  - Industrial: `F_i = max(B_i · C_i(ρ_i) · M_i(D), F_c · (1 + δ))`
- **Telemetry and control loop:** `node/src/fees/lane_pricing.rs` runs the loop each block using utilisation snapshots from `node/src/fees/congestion.rs` and demand signals from `node/src/fees/market_signals.rs`. Gauges expose per-lane utilisation, congestion multipliers, PI adjustments, and applied premiums for dashboards.
- **Governance parameters (defaults live in `governance/src/params.rs`):**

| Parameter (see `node/src/governance/params.rs`) | Default | Meaning |
|---|---|---|
| `lane_consumer_capacity` | 1000 | Nominal consumer throughput used for utilisation `ρ` |
| `lane_industrial_capacity` | 500 | Nominal industrial throughput used for utilisation `ρ` |
| `lane_consumer_congestion_sensitivity` | 300 | `k = 3.0` congestion sensitivity (value / 100) |
| `lane_industrial_congestion_sensitivity` | 500 | `k = 5.0` congestion sensitivity (value / 100) |
| `lane_industrial_min_premium_percent` | 50 | Minimum industrial premium over consumer (percent) |
| `lane_target_utilization_percent` | 70 | PI target utilisation (percent) |
| `lane_market_signal_half_life` | 50 | EMA half-life for demand signal (blocks) |
| `lane_market_demand_max_multiplier_percent` | 300 | Max demand multiplier (percent; 300 = +3.0) |
| `lane_market_demand_sensitivity_percent` | 200 | Demand sensitivity (percent; 200 = 2.0) |
| `lane_pi_proportional_gain_percent` | 10 | PI Kp (percent; 10 = 0.1) |
| `lane_pi_integral_gain_percent` | 1 | PI Ki (percent; 1 = 0.01) |

## Service Badges and Citizenship
- Operators earn service badges when uptime/latency stay within governance thresholds. `node/src/service_badge.rs` calculates eligibility; telemetry publishes `BADGE_ISSUED_TOTAL`, `COMPUTE_PROVIDER_UPTIME`, etc.
- Badges gate governance votes (Operators + Builders houses) and feed range-boost multipliers plus ANN mesh prioritisation.

## Treasury and Disbursements

> **Plain English:** The treasury is the community fund. Moving BLOCK out of it requires a governance vote. Here's the timeline:
>
> ```
> ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌────────────┐    ┌──────────┐    ┌───────────┐
> │  DRAFT  │───▶│ VOTING  │───▶│ QUEUED  │───▶│ TIMELOCKED │───▶│ EXECUTED │───▶│ FINALIZED │
> └─────────┘    └─────────┘    └─────────┘    └────────────┘    └──────────┘    └───────────┘
>      │                                              │                                │
>      │                                              │      ┌────────────┐            │
>      └──────────────────────────────────────────────┴─────▶│ ROLLED BACK│◀───────────┘
>                                                            └────────────┘
> ```
>
> - **Draft**: Someone writes a JSON payload describing where BLOCK should go
> - **Voting**: Bicameral vote (Operators + Builders houses)
> - **Queued**: Passed the vote, waiting for activation
> - **Timelocked**: Waiting period before execution (allows for emergencies)
> - **Executed**: BLOCK actually moves
> - **Finalized**: Done, recorded in ledger
> - **Rolled Back**: Something went wrong; compensation entry created

- Governance proposals now carry explicit treasury-disbursement payloads in addition to param updates. Each disbursement advances through the canonical state machine: **draft → voting → queued → timelocked → executed → finalized/rolled-back**. Drafts are local JSON payloads (stored under `examples/governance/`) validated with `foundation_serialization` schemas before the proposer signs and submits. Voting/timelock rules piggyback on the bicameral governance machinery (see `governance/src/bicameral.rs`), so disbursements inherit quorum, snapshot, and activation semantics.
- Once a disbursement proposal passes, `GovStore` persists the queued entry in sled and snapshots the activation epoch + prior rollbacks to `provenance.json` using first-party encoding (Option A from the task brief). The rollback window remains **block-height bounded** via `governance::store::ROLLBACK_WINDOW_EPOCHS`, guaranteeing deterministic replay on both x86_64 and AArch64.
- Executions emit BLOCK receipts inside the consolidated ledger—no new token types—and every transition (queued, timelocked, executed, rollback) records a ledger journal entry so the explorer and CLI timelines never diverge. Rollbacks simply mark the disbursement as `RolledBack { rolled_back_at, reason }` and append a compensating ledger entry; finalized executions capture the `tx_hash`, execution height, and attested receipt bundle.
- Metrics wiring tracks both balances and pipeline health: `treasury_balance`, `treasury_disbursement_backlog`, and `governance_disbursements_total{status}`. The metrics aggregator exposes `/treasury/summary` and `/governance/disbursements` so dashboards can chart backlog age, quorum wait time, and execution throughput alongside existing treasury gauges. Explorer timelines render the same data (proposal metadata, vote outcomes, timelock window, execution tx, affected accounts, receipts, and rollback annotations).
- **Implementation checklist (AGENTS.md §15.A)** — The governance crate, CLI, explorer, and telemetry stack must:
  1. Extend DAG schemas with multi-stage approvals and attested release bundles (`governance/`, `node/src/governance`, `cli/src/governance`, explorer dashboards).
  2. Emit `/wrappers` metadata whenever treasury diffs occur so operators can diff governance state without scraping sled stores.
  3. Add deterministic replay tests (ledger + `node/tests/`) proving disbursement streaming/rollback stays byte-identical across CPU architectures and `scripts/fuzz_coverage.sh` runs cover the updated code paths.
  4. Update `docs/operations.md`, `docs/apis_and_tooling.md`, and Grafana timelines with “stuck treasury” runbooks, CLI introspection commands, and badge/fee-floor delta overlays.

## Proposal Lifecycle
1. Snapshot of eligible voters occurs on proposal creation (bicameral: Operators + Builders).
2. Secret ballots + timelocks enforced by `governance/src/bicameral.rs`.
3. Parameter changes apply next epoch; upgrades require supermajority plus rollback windows.
4. Emergency catalog/app-layer overrides auto-expire and must be fully logged.

## Governance Parameters
- `governance/src/params.rs` exposes typed knobs for fee floors, multipliers, SLA slashing, telemetry sampling, mesh toggles, AI diagnostics, etc.
- Every integration (node, CLI, explorer, metrics aggregator) uses the same crate so policy proofs line up with on-chain values.
- Historical policy snapshots stream through RPC + CLI; explorers visualise the same baseline.

**Economic + lane pricing parameters (defaults):**

| Param | Default | Notes |
|---|---|---|
| `beta_storage_sub` | 50 | Storage subsidy multiplier |
| `gamma_read_sub` | 20 | Read subsidy multiplier |
| `kappa_cpu_sub` | 10 | Compute subsidy multiplier |
| `lambda_bytes_out_sub` | 5 | Bandwidth subsidy multiplier |
| `read_subsidy_viewer_percent` | 40 | Read subsidy split |
| `read_subsidy_host_percent` | 30 | Read subsidy split |
| `read_subsidy_hardware_percent` | 15 | Read subsidy split |
| `read_subsidy_verifier_percent` | 10 | Read subsidy split |
| `read_subsidy_liquidity_percent` | 5 | Read subsidy split |
| `treasury_percent` | 0 | Treasury share of fees |
| `proof_rebate_limit` | 1 | Max proof rebate share |
| `rent_rate_per_byte` | 0 | Storage rent rate |
| `lane_based_settlement_enabled` | 0 | Enable lane-based settlement routing |
| `lane_consumer_capacity` | 1000 | Consumer capacity |
| `lane_industrial_capacity` | 500 | Industrial capacity |
| `lane_consumer_congestion_sensitivity` | 300 | k = 3.0 |
| `lane_industrial_congestion_sensitivity` | 500 | k = 5.0 |
| `lane_industrial_min_premium_percent` | 50 | Premium floor |
| `lane_target_utilization_percent` | 70 | PI target |
| `lane_market_signal_half_life` | 50 | EMA half-life (blocks) |
| `lane_market_demand_max_multiplier_percent` | 300 | Max demand multiplier |
| `lane_market_demand_sensitivity_percent` | 200 | Demand sensitivity |
| `lane_pi_proportional_gain_percent` | 10 | Kp = 0.1 |
| `lane_pi_integral_gain_percent` | 1 | Ki = 0.01 |

For the full parameter catalog (all governance knobs + defaults), see `docs/system_reference.md#appendix-e--governance-parameter-catalog-partial`, which should mirror the latest entries in `node/src/governance/params.rs`.

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
- CLI: `cli/src/gov.rs` now exposes the disbursement workflow end-to-end: `contract-cli gov disburse create|preview|submit|show|queue|execute|rollback` plus `--schema`/`--check` helpers for JSON payload validation. Existing proposal/DAG helpers remain alongside `cli/src/service_badge.rs` (badge status) and `cli/src/telemetry.rs` (wrapper metadata).
- Explorer + log indexer share the same governance crate via `foundation_serialization` + `foundation_sqlite` wrappers.
- Metrics aggregator publishes `/governance`, `/treasury`, `/wrappers`, and `/bridge` dashboards plus webhook outputs (`docs/operations.md#metrics-aggregator`).

## Ledger Invariants
- Ledger invariants from the former `docs/ledger_invariants.md` now anchor here: no mint-to-EOA, subsidy buckets sum to the recorded total, governance history is monotonic, badge revocations are fully logged, and macro-block anchors must match the gossip replay harness.
