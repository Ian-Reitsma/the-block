# Status & Roadmap

Mainnet readiness: ~97/100 · Vision completion: ~68/100.

The third-token ledger has been fully retired. Every block now mints `STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` in the coinbase, with epoch‑retuned `beta/gamma/kappa/lambda` multipliers smoothing inflation to ≤ 2 %/year. Historical context and migration notes are in [`docs/system_changes.md`](system_changes.md#2024-third-token-ledger-removal-and-ct-subsidy-transition).

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
| **Governance & Subsidy Economy** | **78 %** | Inflation governors tune β/γ/κ/λ multipliers and rent rate; governance can seed reward pools for service roles. | No on-chain treasury or proposal dependencies; grants and multi-stage rollouts remain open. |
| **Consensus & Core Execution** | 74 % | Stake-weighted leader rotation, deterministic tie-breaks, sliding-window difficulty retarget, and parallel executor guard against replay collisions. | Formal proofs still absent. |
| **Smart-Contract VM & UTXO/PoW** | 50 % | Persistent contract store, deployment CLI, and EIP-1559-style fee tracker with BLAKE3 PoW headers. | Opcode library parity and formal VM spec outstanding. |
| **Storage & Free-Read Hosting** | **76 %** | Receipt-only logging, hourly batching, L1 anchoring, and `gateway.reads_since` analytics keep reads free yet auditable. | Incentive-backed DHT storage and offline reconciliation remain prototypes. |
| **Compute Marketplace & CBM** | 65 % | GPU/CPU workloads emit deterministic `ExecutionReceipt`s, compute-unit pricing surfaces in `compute_market.stats`, and redeem curves back CBM. | No heterogeneous scheduling or reputation system; SLA arbitration limited. |
| **Trust Lines & DEX** | 72 % | Authorization-aware trust lines, cost-based multi-hop routing, slippage-checked order books, and on-ledger escrow with partial-payment proofs. Telemetry gauges `dex_escrow_locked`/`dex_escrow_pending`/`dex_escrow_total` track utilisation (total aggregates all escrowed funds). | Cross-chain settlement proofs and advanced routing features outstanding. |
| **Cross-Chain Bridges** | 45 % | Lock/unlock primitives with light-client verification, persisted headers under `state/bridge_headers/`, and CLI deposit/withdraw flows. | Relayer incentives and incentive safety proofs missing. |
| **Wallets, Light Clients & KYC** | 80 % | CLI and hardware wallet support, remote signer workflows, mobile light-client SDKs, and pluggable KYC hooks. | Multisig and production-grade mobile apps outstanding. |
| **Monitoring, Debugging & Profiling** | 67 % | Prometheus/Grafana dashboards expose read-denial and subsidy counters; CLI debugger and profiling utilities ship with nodes. | Bridge/VM metrics and automated anomaly detection missing. |
| **Economic Simulation & Formal Verification** | 35 % | Bench harness simulates inflation/demand; chaos tests capture seeds. | Sparse scenario library and no integrated proof pipeline. |
| **Mobile UX & Contribution Metrics** | 52 % | Background sync and contribution counters respect battery/network constraints. | Push notifications and broad hardware testing pending. |

## Immediate

- Finalize gossip longest-chain convergence, run the chaos harness with 15 % packet loss/200 ms jitter, and document tie-break algorithms and fork-injection fixtures in `docs/networking.md`.
- Retune subsidy multipliers through validator votes (`node/src/rpc/governance.rs`) and expand documentation on inflation parameters.
- Expand settlement audit coverage: index receipts in the explorer (`explorer/indexer.rs`), schedule CI verification jobs, surface mismatches via Prometheus alerts, and ship sample audit reports.
- Harden DHT bootstrapping by persisting peer databases, fuzzing identifier exchange, randomizing bootstrap peer selection, and documenting recovery procedures.
- Broaden fuzz and chaos testing across gateway and storage paths, bound `SimpleDb` bytes to simulate disk-full scenarios, and randomize RPC timeouts for resilience.
- Implement the free-read architecture across gateway and storage: log receipts without charging end users, replenish gateway balances via CT inflation subsidies rather than the retired `read_reward_pool`, enforce token buckets, emit `ExecutionReceipt`s, and update docs/tests to reflect the model. See [system_changes.md](system_changes.md#2024-third-token-ledger-removal-and-ct-subsidy-transition) for historical context.

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

1. **Add rollback tests for PoS finality gadget**
   - Create adversarial forks in `node/tests/finality_rollback.rs`.
   - Simulate conflicting blocks and ensure the finality gadget reorgs correctly.
   - Assert ledger state consistency after rollback.
2. **Implement contract deployment and persistent state**
   - Add deployment transactions in `vm/src/tx.rs` with on-chain storage.
   - Persist contract key/value state under `state/contracts/`.
   - Write execution tests confirming state survives restarts.
3. **Ship ABI tooling and contract CLI**
   - Generate ABI files from `vm/src/opcodes.rs`.
   - Expose `contract deploy/call` commands in `cli/`.
   - Document usage in `docs/contract_dev.md`.
4. **Introduce dynamic gas fee market**
   - Track base fee per block in `node/src/fees.rs`.
   - Implement EIP-1559-style adjustment based on block fullness.
   - Update mempool to reject under-priced transactions.
5. **Bridge UTXO and account models**
   - Add translation layer in `ledger/src/utxo_account.rs`.
   - Ensure spent UTXOs update account balances atomically.
   - Provide migration tools for existing balances.
6. **Merge PoW blocks into PoS finality path**
   - Allow PoW headers to reference PoS checkpoints in `consensus/src/pow.rs`.
   - Update fork-choice to prefer finalized PoS chain with valid PoW.
   - Add regression tests covering mixed PoW/PoS chains.
7. **Build fee-aware mempool**
   - Sort pending transactions by effective fee in `node/src/mempool.rs`.
   - Evict low-fee transactions when capacity exceeds threshold.
   - Ensure higher-fee transactions are processed first in tests.
8. **Enhance multi-hop trust-line routing**
   - Implement cost-based path scoring in `trust_lines/src/path.rs`.
   - Add fallback routes when optimal paths fail mid-transfer.
   - Update documentation with routing algorithm details.
9. **Expose subsidy parameter proposals in gov-ui**
    - List multiplier and rent-rate proposals in the UI.
    - Allow voting and activation through web interface.
    - Sync results via `governance/params.rs`.
10. **Index settlement receipts in explorer storage**
    - Parse receipt files in `explorer/indexer.rs`.
    - Persist anchors and issuance events into explorer DB.
    - Add REST endpoints to query finalized batches.
11. **Schedule settlement verification in CI**
    - Add a CI job invoking `settlement.audit`.
    - Fail builds on mismatched anchors or subsidy totals.
    - Provide sample configs in `ci/settlement.yml`.
12. **Fuzz peer identifier parsing**
    - Create fuzz target for `net/discovery.rs` identifier parser.
    - Integrate with `cargo-fuzz` under `fuzz/`.
    - Run in CI with crash-on-error.
13. **Document manual DHT recovery procedures**
    - Write `docs/dht_recovery.md` with step-by-step commands.
       - Include troubleshooting for stale peer lists.
    - Cross-reference `net/discovery.rs` comments.
14. **Integrate gateway fuzzing**
    - Build fuzz harness for `gateway/http.rs` request handling.
    - Seed with realistic HTTP traffic patterns.
    - Wire into nightly CI runs.
15. **Simulate disk exhaustion in storage tests**
    - Modify `node/tests/storage_repair.rs` to limit tmpfs size.
    - Validate graceful error and recovery paths.
    - Ensure receipts and ledger updates remain consistent.
16. **Randomize RPC client timeouts**
    - Introduce jitter in `node/src/rpc/client.rs` timeout settings.
    - Expose config knob `rpc.timeout_jitter_ms`.
    - Test under high latency to confirm resilience.
17. **Add push notification hooks for subsidy events**
    - Emit webhook or FCM triggers in wallet tooling.
    - Allow mobile clients to register tokens via RPC.
    - Document opt-in flow in `docs/mobile.md`.
18. **Set up formal verification for consensus rules**
    - Translate the state machine into F* modules under `formal/consensus`.
    - Create CI jobs running `fstar` to ensure proofs compile.
    - Provide developer guide in `formal/README.md`.
