# Status & Roadmap

Mainnet readiness: ~99.6/100 · Vision completion: ~84.2/100.

The third-token ledger has been fully retired. Every block now mints `STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` in the coinbase, with epoch‑retuned `beta/gamma/kappa/lambda` multipliers smoothing inflation to ≤ 2 %/year. Fleet-wide peer metrics feed a dedicated `metrics-aggregator`, the scheduler supports graceful `compute.job_cancel` rollbacks, fee-floor policy changes persist into `GovStore` history with rollback hooks and telemetry, and DID anchors flow through explorer APIs for cross-navigation with wallet addresses. Historical context and migration notes are in [`docs/system_changes.md`](system_changes.md#2024-third-token-ledger-removal-and-ct-subsidy-transition).

## Economic Model Snapshot

Every subsidy bucket follows a one‑dial multiplier formula driven by realised
utilisation:

\[
\text{multiplier}_x = \frac{\phi_x I_{\text{target}} S / 365}{U_x / \text{epoch\_secs}}
\]

Adjustments clamp to ±15 % of the previous value; if usage `U_x` approaches
zero, the last multiplier doubles to keep incentives alive. Base miner rewards
shrink with the effective miner count via a logistic curve

\[
R_0(N) = \frac{R_{\max}}{1 + e^{\xi (N - N^\star)}}
\]

with hysteresis `ΔN ≈ √N*` to damp flash joins and leaves. Full derivations and
worked examples live in [`docs/economics.md`](economics.md).

For a subsystem-by-subsystem breakdown with evidence and remaining gaps, see
[docs/progress.md](progress.md).

## Strategic Pillars

| Pillar | % Complete | Highlights | Gaps |
| --- | --- | --- | --- |
| **Governance & Subsidy Economy** | **90 %** | Inflation governors tune β/γ/κ/λ multipliers and rent rate; multi-signature release approvals, attested fetch/install tooling, fee-floor policy timelines, and DID revocation history are archived in `GovStore` alongside CLI telemetry with rollback support. | No on-chain treasury or proposal dependencies; grants and multi-stage rollouts remain open. |
| **Consensus & Core Execution** | 86 % | Stake-weighted leader rotation, deterministic tie-breaks, multi-window Kalman difficulty retune, release rollback helpers, and parallel executor guard against replay collisions. | Formal proofs still absent. |
| **Smart-Contract VM & UTXO/PoW** | 79 % | Persistent contract store, deterministic WASM runtime with debugger, and EIP-1559-style fee tracker with BLAKE3 PoW headers. | Opcode library parity and formal VM spec outstanding. |
| **Storage & Free-Read Hosting** | **79 %** | Receipt-only logging, hourly batching, L1 anchoring, and `gateway.reads_since` analytics keep reads free yet auditable. | Incentive-backed DHT storage and offline reconciliation remain prototypes. |
| **Networking & Gossip** | 93 % | QUIC mutual-TLS rotation with diagnostics/chaos harnesses, cluster `metrics-aggregator`, partition watch with gossip markers, and CLI/RPC metrics via `net.peer_stats`. | Large-scale WAN chaos tests outstanding. |
| **Compute Marketplace & CBM** | 80 % | Capability-aware scheduler weights offers by reputation, matches GPU/CPU requirements, enforces fee floors with per-sender slots, and surfaces governance-tuned fee-floor windows/percentiles to wallets and telemetry. | Escrowed payments and SLA enforcement remain rudimentary. |
| **Trust Lines & DEX** | 78 % | Authorization-aware trust lines, cost-based multi-hop routing, slippage-checked order books, and on-ledger escrow with partial-payment proofs. Telemetry gauges `dex_escrow_locked`/`dex_escrow_pending`/`dex_escrow_total` track utilisation (total aggregates all escrowed funds). | Cross-chain settlement proofs and advanced routing features outstanding. |
| **Cross-Chain Bridges** | 48 % | Lock/unlock primitives with light-client verification, persisted headers under `state/bridge_headers/`, and CLI deposit/withdraw flows. | Relayer incentives and incentive safety proofs missing. |
| **Wallets, Light Clients & KYC** | 91 % | CLI and hardware wallet support, remote signer workflows, mobile light-client SDKs, session-key delegation, auto-update orchestration, fee-floor caching with localized warnings/JSON output, telemetry-backed QoS overrides, and pluggable KYC hooks. | Multisig wallet UX and production-grade mobile apps outstanding. |
| **Monitoring, Debugging & Profiling** | 85 % | Prometheus/Grafana dashboards, metrics-to-logs correlation with automated QUIC dumps, VM trace counters, DID anchor gauges, and CLI debugger/profiling utilities ship with nodes; wallet QoS events and fee-floor rollbacks now plot alongside DID timelines. | Bridge/VM anomaly detection still pending. |
| **Identity & Explorer** | 78 % | DID registry anchors with replay protection and optional provenance attestations, wallet and light-client commands support anchoring/resolving with sign-only/remote signer flows, explorer `/dids` endpoints expose history/anchor-rate charts with LRU caching, and governance archives revocation history alongside anchor data for audit. | Governance-driven revocation playbooks and mobile identity UX remain to ship. |
| **Economic Simulation & Formal Verification** | 38 % | Bench harness simulates inflation/demand; chaos tests capture seeds. | Sparse scenario library and no integrated proof pipeline. |
| **Mobile UX & Contribution Metrics** | 56 % | Background sync and contribution counters respect battery/network constraints. | Push notifications and broad hardware testing pending. |

## Immediate

- **Run fleet-scale QUIC chaos drills** – invoke `scripts/chaos.sh --quic-loss 0.15 --quic-dup 0.03` across multi-region clusters, harvest retransmit deltas via `sim/quic_chaos_summary.rs`, and extend `docs/networking.md` with mitigation guidance drawn from the new telemetry.
- **Draft governance treasury and dependency RFC** – prototype ledger tables for queued payouts, encode proposal prerequisites in `node/src/governance/store.rs`, and capture the operational playbook in `docs/governance.md`.
- **Automate release rollout alerting** – add explorer jobs that reconcile `release_history` installs against the signer threshold, publish Grafana panels for stale nodes, and raise alerts when `release_quorum_fail_total` moves without a corresponding signer update.
- **Stand up anomaly heuristics in the aggregator** – feed correlation caches into preliminary anomaly scoring, auto-request log dumps on clustered `quic_handshake_fail_total{peer}` spikes, and document the response workflow in `docs/monitoring.md`.
- **Ship operator rollback drills** – expand `docs/governance_release.md` with staged rollback exercises that rehearse `update::rollback_failed_startup`, including guidance for restoring prior binaries and verifying provenance signatures after a revert.
- **Operationalize DID anchors** – wire revocation alerts into explorer dashboards, expand `docs/identity.md` with recovery guidance, and ensure wallet/light-client flows surface governance revocations before submitting new anchors.

## Near Term

- **Industrial lane SLA enforcement and dashboard surfacing** – enforce deadline slashing for tardy providers, track ETAs and on-time percentages, visualize payout caps, and ship operator remediation guides.
- **Range-boost mesh trials and mobile energy heuristics** – prototype BLE/Wi-Fi Direct relays, tune lighthouse multipliers via field energy usage, log mobile battery/CPU metrics, and publish developer heuristics.
- **Economic simulator runs for emission/fee policy** – parameterize inflation/demand scenarios, run Monte Carlo batches via bench-harness, report top results to governance, and version-control scenarios.
- **Compute-backed money and instant-app groundwork** – define redeem curves for CBM, prototype local instant-app execution hooks, record resource metrics for redemption, test edge cases, and expose CLI plumbing.

## Medium Term

- **Full cross-chain exchange routing** – implement adapters for SushiSwap and Balancer, integrate bridge fee estimators and route selectors, simulate multi-hop slippage, watchdog stuck swaps, and document guarantees.
- **Distributed benchmark network at scale** – deploy harness across 100+ nodes/regions, automate workload permutations, gather latency/throughput heatmaps, generate regression dashboards, and publish tuning guides.
- **Wallet ecosystem expansion** – add multisig modules, ship Swift/Kotlin SDKs, enable hardware wallet firmware updates, provide backup/restore tooling, and host interoperability tests.
- **Governance feature extensions** – roll out staged upgrade pipelines, support proposal dependencies and queue management, add on-chain treasury accounting, offer community alerts, and finalize rollback simulation playbooks.
- **Mobile light client productionization** – optimize header sync/storage, add push notification hooks for subsidy events, integrate background energy-saving tasks, support mobile signing, and run a cross-hardware beta program.

## Long Term

- **Smart-contract VM and SDK release** – design a deterministic instruction set with gas accounting, ship developer tooling and ABI specs, host example apps, audit and formally verify the stack.
- **Permissionless compute marketplace** – integrate heterogeneous GPU/CPU scheduling, enable provider reputation scoring, support escrowed cross-chain payments, build an SLA arbitration framework, and release marketplace analytics.
- **Global jurisdiction compliance framework** – publish regional policy packs, add PQ encryption, maintain transparency logs, allow per-region feature toggles, and run forkability trials.
- **Decentralized storage and bandwidth markets** – incentivize DHT storage, reward long-range mesh relays, integrate content addressing, benchmark large file transfers, and provide retrieval SDKs.
- **Mainnet launch and sustainability** – lock protocol parameters via governance, run multi-phase audits and bug bounties, schedule staged token releases, set up long-term funding mechanisms, and establish community maintenance committees.

## Next Tasks

1. **Implement governance treasury accounting**
   - Extend `node/src/governance/store.rs` with a `treasury_balances` table and checkpointed accruals.
   - Surface balances and disbursements via `rpc/governance.rs` plus CLI reporting.
   - Add regression coverage in `governance/tests/treasury_flow.rs` to confirm replay safety.
2. **Add proposal dependency resolution**
   - Encode prerequisite DAG edges in `node/src/governance/mod.rs` and persist them to the store.
   - Block activation in `controller::submit_release` until dependencies clear, logging failures through `release_quorum_fail_total`.
   - Document the workflow in `docs/governance.md` with explorer examples.
3. **Scale the QUIC chaos harness**
   - Allow `node/tests/quic_chaos.rs` to spawn multi-node meshes with seeded RNGs.
   - Export aggregated retransmit stats to `sim/quic_chaos_summary.rs` and archive representative traces for future tuning.
   - Update `scripts/chaos.sh` to accept topology manifests for repeatable WAN drills.
4. **Automate release rollout alerting**
   - Add an explorer cron that snapshots `release_history` and highlights nodes lagging more than one epoch.
   - Publish Grafana panels powered by `release_installs_total` and signer metadata.
   - Emit webhook alerts when installs stall beyond configurable thresholds.
5. **Stand up anomaly heuristics in the aggregator**
   - Feed correlation caches into a pluggable anomaly scoring engine within `metrics-aggregator`.
   - Persist annotations for later audit and surface them over the REST API.
   - Backstop behaviour with tests in `metrics-aggregator/tests/correlation.rs`.
6. **Enforce compute-market SLAs**
   - Introduce deadline tracking in `node/src/compute_market/scheduler.rs` and penalize tardy providers.
   - Record `compute_sla_violation_total` metrics and integrate with the reputation store.
   - Document remediation expectations in `docs/compute_market.md`.
7. **Prototype incentive-backed DHT storage**
   - Extend `storage_market` to price replicas, tracking deposits and proofs in `storage_market/src/lib.rs`.
   - Add explorer visibility into outstanding storage contracts and payouts.
   - Simulate churn within the `sim` crate to calibrate incentives before deployment.
8. **Deliver multisig wallet UX**
   - Layer multisig account abstractions into `crates/wallet` with CLI flows for key rotation and spending policies.
   - Ensure remote signer compatibility and persistence across upgrades.
   - Update `docs/wallets.md` with operator and end-user runbooks.
9. **Extend cross-chain settlement proofs**
   - Implement proof verification for additional partner chains in `bridges/src/light_client.rs`.
   - Capture incentives and slashable behaviour for relayers in `bridges/src/relayer.rs`.
   - Document settlement guarantees and failure modes in `docs/bridges.md`.
10. **Kick off formal verification pipeline**
    - Translate consensus rules into F* modules under `formal/consensus` with stub proofs.
    - Integrate proof builds into CI with caching to keep feedback fast.
    - Publish contributor guidelines in `formal/README.md` and schedule brown-bag sessions for new authors.
