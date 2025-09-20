# Project Progress Snapshot

This document tracks high‑fidelity progress across The‑Block's major work streams.  Each subsection lists the current completion estimate, supporting evidence with canonical file or module references, and the remaining gaps.  Percentages are rough, *engineer-reported* gauges meant to guide prioritization rather than marketing claims.

Mainnet readiness currently measures **~99.6/100** with vision completion **~86.9/100**. Subsidy accounting now lives solely in the unified CT ledger; see `docs/system_changes.md` for migration notes. The standalone `governance` crate mirrors the node state machine for CLI/SDK use, the compute marketplace persists CT/IT flows in a RocksDB ledger with activation metadata, Merkle roots, and RPC/telemetry exposure for audits, wallet binaries share a unified `ed25519-dalek 2.2.x` stack with multisig signer telemetry, and the RPC client clamps `TB_RPC_FAULT_RATE` while saturating exponential backoff to avoid overflow and restore environment state on drop. Remaining focus areas: deliver treasury disbursement tooling, harden compute-market SLAs with dashboards, extend bridge/DEX docs with signer-set payloads and release-verifier guidance, continue WAN-scale QUIC chaos drills, and polish multisig UX. Subsidy multipliers retune each epoch via the one‑dial formula

\[
\text{multiplier}_x = \frac{\phi_x I_{\text{target}} S / 365}{U_x / \text{epoch\_secs}}
\]

clamped to ±15 % of the previous value. Base miner rewards decrease as the effective miner count rises following

\[
R_0(N) = \frac{R_{\max}}{1 + e^{\xi (N - N^\star)}}
\]

with hysteresis `ΔN ≈ √N*` to blunt flash joins. Full derivations live in [`docs/economics.md`](economics.md). The canonical roadmap with near‑term tasks lives in [`docs/roadmap.md`](roadmap.md).

## 1. Consensus & Core Execution — ~89 %

**Evidence**
- Hybrid PoW/PoS chain: `node/src/consensus/pow.rs` embeds PoS checkpoints and `node/src/consensus/fork_choice.rs` prefers finalized chains.
- Kalman-weighted multi-window difficulty retune with `retune_hint` metrics in `node/src/consensus/difficulty_retune.rs` and `docs/difficulty.md`.
- Rollback checkpoints and partition recovery hooks in `node/src/consensus/fork_choice.rs` and `node/tests/partition_recovery.rs`.
- EIP‑1559 base fee tracker: `node/src/fees.rs` adjusts per block and `node/tests/base_fee.rs` verifies target fullness tracking.
- Adversarial rollback tests in `node/tests/finality_rollback.rs` assert ledger state after competing forks.
- Pietrzak VDF with timed commitment and delayed preimage reveal (`node/src/consensus/vdf.rs`, `node/tests/vdf.rs`) shrinks proofs and blocks speculative computation.
- Hadamard–Unruh committee sampler with Count-Sketch top‑k (`node/src/consensus/hadamard.rs`, `node/src/consensus/committee/topk.rs`).
- Sequential BLAKE3 proof-of-history ticker with optional GPU offload (`node/src/poh.rs`, `node/tests/poh.rs`). See `docs/poh.md`.
- Dilithium-based commit–reveal path with nonce replay protection (`node/src/commit_reveal.rs`, `node/tests/commit_reveal.rs`) compresses blind signatures and thwarts mempool DoS. See `docs/commit_reveal.md` for design details.
- Heisenberg + VDF fuse (`node/src/consensus/vdf.rs`) enforces a ≥2-block delay before randomness-dependent transactions execute.
- Parallel executor and transaction scheduler document concurrency guarantees (`docs/scheduler.md`, `node/src/parallel.rs`, `node/src/scheduler.rs`).
- Transaction lifecycle, memo handling, and dual fee lanes documented in `docs/transaction_lifecycle.md`.
- Macro-block checkpointing and per-shard fork choice preserve cross-shard ordering (`node/src/blockchain/macro_block.rs`, `node/src/blockchain/shard_fork_choice.rs`).

**Gaps**
- Formal safety/liveness proofs under `formal/` still stubbed.
- No large‑scale network rollback simulation.

## 2. Networking & Gossip — ~95 %

**Evidence**
- Deterministic gossip with partition tests: `node/tests/net_gossip.rs` and docs in `docs/networking.md`.
- QUIC transport with mutual-TLS certificate rotation, cached diagnostics, TCP fallback, and mixed-transport fanout; integration covered in `node/tests/net_quic.rs`, `node/src/net/transport_quic.rs`, and `docs/network_quic.md`, with telemetry via `quic_cert_rotation_total` and per-peer `quic_retransmit_total`/`quic_handshake_fail_total` counters.
- `net.quic_stats` RPC and `contract-cli net quic-stats` expose cached latency,
  retransmit, and endpoint reuse data with per-peer failure metrics for operators.
- TTL-based duplicate suppression and sqrt-N fanout documented in `docs/gossip.md` and implemented in `node/src/gossip/relay.rs`.
  - Peer identifier fuzzing prevents malformed IDs from crashing DHT routing (`net/fuzz/peer_id.rs`).
  - Manual DHT recovery runbook (`docs/networking.md#dht-recovery`).
  - Peer database and chunk cache persist across restarts with configurable paths (`node/src/net/peer.rs` via `TB_PEER_DB_PATH` and `TB_CHUNK_DB_PATH`); `TB_PEER_SEED` fixes shuffle order for reproducible bootstraps.
  - ASN-aware A* routing oracle (`node/src/net/a_star.rs`) chooses k cheapest paths per shard and feeds compute-placement SLAs.
  - SIMD Xor8 rate-limit filter with AVX2/NEON dispatch (`node/src/web/rate_limit.rs`, `docs/benchmarks.md`) handles 1 M rps bursts.
  - Jittered JSON‑RPC client with exponential backoff (`node/src/rpc/client.rs`) prevents thundering-herd reconnect storms.
  - Gateway DNS publishing and policy retrieval logged in `docs/gateway_dns.md` and implemented in `node/src/gateway/dns.rs`.
    - Per-peer rate-limit telemetry and reputation tracking via `net.peer_stats` RPC and `net stats` CLI, capped by `max_peer_metrics`.
     - Partition watch detects split-brain conditions and stamps gossip with markers (`node/src/net/partition_watch.rs`, `node/src/gossip/relay.rs`).
     - Cluster-wide metrics pushed to the `metrics-aggregator` crate for fleet visibility.
    - Shard-aware peer maps and gossip routing limit block broadcasts to interested shards (`node/src/gossip/relay.rs`).
    - Uptime-based fee rebates tracked in `node/src/net/uptime.rs` with `peer.rebate_status` RPC (`docs/fee_rebates.md`).

**Gaps**
- Large-scale WAN chaos experiments remain open.
- Bootstrap peer churn analysis missing.

## 3. Governance & Subsidy Economy — ~92 %

**Evidence**
- Subsidy multiplier proposals surfaced via `node/src/rpc/governance.rs` and web UI (`tools/gov-ui`).
- Shared `governance` crate re-exports bicameral voting, sled-backed `GovStore`, proposal DAG validation, Kalman retune helpers, and release workflows for CLI/SDK consumers (`governance/src/lib.rs` and examples).
- Push notifications on subsidy balance changes (`wallet` tooling).
- Explorer indexes settlement receipts with query endpoints (`explorer/src/lib.rs`).
- Risk-sensitive Kalman–LQG governor with variance-aware smoothing (`node/src/governance/kalman.rs`, `node/src/governance/variance.rs`).
- Laplace-noised multiplier releases and miner-count logistic hysteresis (`node/src/governance/params.rs`, `pow/src/reward.rs`).
- Emergency kill switch `kill_switch_subsidy_reduction` with telemetry counters (`node/src/governance/params.rs`, `docs/monitoring.md`).
- Subsidy accounting is unified in the CT ledger with migration documented in `docs/system_changes.md`.
- Multi-signature release approvals persist signer sets and thresholds (`node/src/governance/release.rs`), gated fetch/install flows (`node/src/update.rs`, `cli/src/gov.rs`), and explorer/CLI timelines (`explorer/src/release_view.rs`, `contract explorer release-history`).
- Telemetry counters `release_quorum_fail_total` and `release_installs_total` expose quorum health and rollout adoption for dashboards.
- Fee-floor window and percentile parameters (`node/src/governance/params.rs`) stream through `GovStore` history with rollback support (`node/src/governance/store.rs`), governance CLI updates (`cli/src/gov.rs`), explorer timelines (`explorer/src/lib.rs`), and regression coverage (`governance/tests/mempool_params.rs`).
- DID revocations share the same `GovStore` history and prevent further anchors until governance clears the entry; the history is available to explorer and wallet tooling so revocation state can be surfaced alongside DID records (`node/src/governance/store.rs`, `node/src/identity/did.rs`, `docs/identity.md`).
- Simulations `sim/release_signers.rs` and `sim/lagging_release.rs` model signer churn and staggered downloads to validate quorum durability and rollback safeguards before production deployment.
- One‑dial multiplier formula retunes β/γ/κ/λ per epoch using realised utilisation `U_x`, clamped to ±15 % and doubled when `U_x` → 0; see `docs/economics.md`.
- Demand gauges `industrial_backlog` and `industrial_utilization` feed
    `Block::industrial_subsidies()` and surface via `inflation.params` and
    `compute_market.stats`.
- Arbitrary CT/IT fee splits tracked by `pct_ct`; `reserve_pending` debits
    balances before coinbase accumulation, documented in `docs/fees.md`.
- Logistic base reward `R_0(N) = R_max / (1 + e^{ξ (N - N^*)})` with hysteresis `ΔN ≈ √N*` dampens miner churn and is implemented in `pow/src/reward.rs`.
 - Kalman filter weights for difficulty retune configurable via governance parameters (`node/src/governance/params.rs`).

**Gaps**
- Publish explorer timelines for proposal windows and upcoming treasury disbursements emitted by the CLI/governance crate.
- No on‑chain treasury or proposal dependency system.
- Governance rollback simulation incomplete.

## 4. Storage & Free‑Read Hosting — ~83 %

**Evidence**
- Read acknowledgement batching and audit flow documented in `docs/read_receipts.md` and `docs/storage_pipeline.md`.
- Disk‑full metrics and recovery tests (`node/tests/storage_disk_full.rs`).
- Gateway HTTP parsing fuzz harness (`gateway/fuzz`).
- RaptorQ progressive fountain overlay for BLE repair (`node/src/storage/repair.rs`, `docs/storage/repair.md`, `node/tests/raptorq_repair.rs`).
- Thread-safe `ReadStats` telemetry and analytics RPC (`node/src/telemetry.rs`, `node/tests/analytics.rs`).
- WAL-backed `SimpleDb` design in `docs/simple_db.md` underpins DNS cache, chunk gossip, and DEX storage.
- Base64 snapshots stage through `NamedTempFile::persist` plus `sync_all`, with legacy dumps removed only after durable rename (`node/src/simple_db/memory.rs`, `node/tests/simple_db/memory_tests.rs`).
- Rent escrow metrics (`rent_escrow_locked_ct_total`, etc.) exposed in `docs/monitoring.md` with alert thresholds.
- Reputation-weighted Lagrange allocation and proof-of-retrievability challenges secure storage contracts (`node/src/gateway/storage_alloc.rs`, `storage/src/contract.rs`).

**Gaps**
- Incentive‑backed DHT storage marketplace still conceptual.
- Offline escrow reconciliation absent.

## 5. Smart‑Contract VM & UTXO/PoW — ~82 %

**Evidence**
- Persistent `ContractStore` with CLI deploy/call flows (`state/src/contracts`, `cli/src/main.rs`).
- ABI generation from opcode enum (`node/src/vm/opcodes.rs`).
- State survives restarts (`node/tests/vm.rs::state_persists_across_restarts`).
- Planned dynamic gas fee market (`node/src/fees.rs` roadmap) anchors eventual EIP-1559 adaptation.
- Deterministic WASM runtime with fuel-based metering and ABI helpers (`node/src/vm/wasm/mod.rs`, `node/src/vm/wasm/gas.rs`).
- Interactive debugger and trace export (`node/src/vm/debugger.rs`, `docs/vm_debugging.md`).

**Gaps**
- Instruction set remains minimal; no formal VM spec or audits.
- Developer SDK and security tooling pending.

## 6. Compute Marketplace & CBM — ~88 %

**Evidence**
- Deterministic GPU/CPU hash runners (`node/src/compute_market/workloads`).
- `compute.job_cancel` RPC releases resources and refunds bonds (`node/src/rpc/compute_market.rs`).
- Capability-aware scheduler matches CPU/GPU workloads, weights offers by provider reputation, and handles cancellations (`node/src/compute_market/scheduler.rs`).
- Price board persistence with metrics (`docs/compute_market.md`).
- Settlement persists CT/IT balances, audit logs, activation metadata, and Merkle roots in a RocksDB-backed store with RPC/CLI/explorer surfacing (`node/src/compute_market/settlement.rs`, `node/tests/compute_settlement.rs`, `docs/compute_market.md`, `docs/settlement_audit.md`, `explorer/src/compute_view.rs`). The ledger emits telemetry (`SETTLE_APPLIED_TOTAL`, `SETTLE_FAILED_TOTAL{reason}`, `SETTLE_MODE_CHANGE_TOTAL{state}`, `SLASHING_BURN_CT_TOTAL`, `COMPUTE_SLA_VIOLATIONS_TOTAL{provider}`) and exposes `compute_market.provider_balances`, `compute_market.audit`, and `compute_market.recent_roots` RPCs for automated reconciliation.
- `Settlement::shutdown` persists any pending ledger deltas and flushes RocksDB handles before teardown so test harnesses (and unplanned exits) leave behind consistent CT/IT balances and Merkle roots for replay.
- Admission enforces dynamic fee floors with per-sender slot caps, eviction audit trails, explorer charts, and `mempool.stats` exposure (`node/src/mempool/admission.rs`, `node/src/mempool/scoring.rs`, `docs/mempool_qos.md`, `node/tests/mempool_eviction.rs`). Governance parameters for the floor window and percentile stream through telemetry (`fee_floor_window_changed_total`, `fee_floor_warning_total`, `fee_floor_override_total`) and wallet guidance.
- `FeeFloor::new(size, percentile)` now requires explicit percentile inputs in tests and CLI paths, aligning mempool QoS regressions with governance-configured sampling windows (`node/src/mempool/scoring.rs`, `node/tests/mempool_qos.rs`).
- Economic simulator outputs KPIs to CSV (`sim/src`).
- Durable courier receipts with exponential backoff documented in `docs/compute_market_courier.md` and implemented in `node/src/compute_market/courier.rs`.
- Groth16/Plonk SNARK verification for compute receipts (`node/src/compute_market/snark.rs`).

**Gaps**
- Escrowed payments and automated SLA enforcement remain rudimentary; deadline tracking and slashing heuristics are staged but not yet active.

## 7. Trust Lines & DEX — ~81 %

**Evidence**
- Persistent order books via `node/src/dex/storage.rs` and restart tests (`node/tests/dex_persistence.rs`).
- Cost‑based multi‑hop routing with fallback paths (`node/src/dex/trust_lines.rs`).
- On-ledger escrow with partial-payment proofs (`dex/src/escrow.rs`, `node/tests/dex.rs`, `dex/tests/escrow.rs`).
- Trade logging and routing semantics documented in `docs/dex.md`.
- CLI escrow flows and Merkle proof verification exposed via `dex escrow status`/
  `dex escrow release` commands and `dex.escrow_proof` RPC. Telemetry gauges
  `dex_escrow_locked`, `dex_escrow_pending`, and `dex_escrow_total` monitor
  utilisation; `dex_escrow_total` aggregates locked funds across all escrows.
- Constant-product AMM pools and liquidity mining incentives (`dex/src/amm.rs`, `docs/dex_amm.md`).

**Gaps**
- Escrow for cross‑chain DEX routes absent.

## 8. Wallets, Light Clients & KYC — ~94 %

**Evidence**
- CLI + hardware wallet support (`crates/wallet`).
- Remote signer workflows (`crates/wallet/src/remote_signer.rs`, `docs/wallets.md`).
- Mobile light client with push notification hooks (`examples/mobile`, `docs/mobile_light_client.md`).
- Light-client synchronization and header verification documented in `docs/light_client.md`.
- Real-time state streaming over WebSockets with zstd snapshots (`docs/light_client_stream.md`, `node/src/rpc/state_stream.rs`).
- Optional KYC provider wiring (`docs/kyc.md`).
- Session-key issuance and meta-transaction tooling (`crypto/src/session.rs`, `cli/src/wallet.rs`, `docs/account_abstraction.md`).
- Telemetry `session_key_issued_total`/`session_key_expired_total` and simulator churn knob (`sim/src/lib.rs`).
- Release fetch/install tooling verifies provenance, records timestamps, and exposes explorer/CLI history for operator audits (`node/src/update.rs`, `cli/src/gov.rs`, `explorer/src/release_view.rs`).
- Wallet send flow caches fee-floor lookups, emits localized warnings with auto-bump or `--force` overrides, streams telemetry events back to the node, and exposes JSON mode for automation (`cli/src/wallet.rs`, `docs/mempool_qos.md`).
- Unified `ed25519-dalek 2.2.x` signature handling ensures remote signer payloads, CLI staking flows, and explorer attestations all share compatible types while forwarding multisig signer arrays and escrow hash algorithms (`crates/wallet`, `node/src/bin/wallet.rs`, `tests/remote_signer_multisig.rs`).
- Remote signer metrics (`remote_signer_request_total`, `remote_signer_success_total`, `remote_signer_error_total{reason}`) integrate with wallet QoS counters so dashboards highlight signer outages alongside fee-floor overrides (`crates/wallet/src/remote_signer.rs`, `docs/monitoring.md`).

**Gaps**
- Polish multisig UX (batched signer discovery, richer operator prompts) before tagging the next CLI release.
- Surface multisig signer history in explorer/CLI output for auditability.
- Production‑grade mobile apps not yet shipped.

## 9. Bridges & Cross‑Chain Routing — ~52 %

**Evidence**
- Lock/unlock bridge contract with relayer proofs (`bridges/src/lib.rs`).
- Light-client verification checks foreign headers (`docs/bridges.md`).
- CLI deposit/withdraw flows (`cli/src/main.rs` subcommands).
- Hardened HTLC script parsing supports SHA3 and RIPEMD encodings (`bridges/src/lib.rs`).
- Bridge walkthrough in `docs/bridges.md`.

**Gaps**
- Relayer incentive mechanisms undeveloped.
- No safety audits or circuit proofs.

## 10. Monitoring, Debugging & Profiling — ~88 %

**Evidence**
  - Prometheus exporter with extensive counters (`node/src/telemetry.rs`).
  - Service badge tracker exports uptime metrics and RPC status (`node/src/service_badge.rs`, `node/tests/service_badge.rs`). See `docs/service_badge.md`.
  - Monitoring stack via `make monitor` and docs in `docs/monitoring/README.md`.
    - Cluster metrics aggregation with disk-backed retention (`metrics-aggregator` crate).
    - Metrics-to-logs correlation links Prometheus anomalies to targeted log dumps and exposes `log_correlation_fail_total` for missed lookups (`metrics-aggregator/src/lib.rs`, `node/src/rpc/logs.rs`, `cli/src/logs.rs`).
    - VM trace counters and partition dashboards (`node/src/telemetry.rs`, `monitoring/templates/partition.json`).
    - Settlement audit CI job (`.github/workflows/ci.yml`).
    - Fee-floor policy changes and wallet overrides surface via `fee_floor_window_changed_total`, `fee_floor_warning_total`, and `fee_floor_override_total`, while DID anchors increment `did_anchor_total` for explorer dashboards (`node/src/telemetry.rs`, `monitoring/metrics.json`, `docs/mempool_qos.md`, `docs/identity.md`).
    - Incremental log indexer resumes from offsets, rotates encryption keys, streams over WebSocket, and exposes REST filters (`tools/log_indexer.rs`, `docs/logging.md`).

**Gaps**
- Bridge and VM metrics are sparse.
- Automated anomaly detection not in place.

## 11. Identity & Explorer — ~80 %

**Evidence**
- DID registry persists anchors with replay protection, governance revocation checks, and optional provenance attestations (`node/src/identity/did.rs`, `state/src/did.rs`).
- Light-client commands anchor and resolve DIDs with remote signer support, sign-only payload export, and JSON output for automation (`cli/src/light_client.rs`, `examples/did.json`).
- Explorer ingests DID updates into `did_records`, serves `/dids`, `/identity/dids/:address`, and anchor-rate metrics for dashboards (`explorer/src/did_view.rs`, `explorer/src/main.rs`).
- Explorer caches DID lookups in-memory to avoid redundant RocksDB reads and drives anchor-rate dashboards from `/dids/metrics/anchor_rate` (`explorer/src/did_view.rs`, `explorer/src/main.rs`).
- Governance history captures fee-floor and DID revocations for auditing alongside wallet telemetry (`node/src/governance/store.rs`, `docs/identity.md`).

**Gaps**
- Revocation alerting and recovery runbooks need explorer/CLI integration.
- Mobile wallet identity UX and bulk export tooling remain outstanding.

## 12. Economic Simulation & Formal Verification — ~41 %

**Evidence**
- Simulation scenarios for inflation/demand/backlog (`sim/src`).
- F* scaffolding for consensus proofs (`formal/` installers and docs).
- Scenario library exports KPIs to CSV.

**Gaps**
- Formal proofs beyond scaffolding missing.
- Scenario coverage still thin.

## 13. Mobile UX & Contribution Metrics — ~59 %

**Evidence**
- Background sync respecting battery/network constraints (`docs/mobile_light_client.md`).
- Contribution metrics and optional KYC in mobile example (`examples/mobile`).
- Push notifications for subsidy events (wallet tooling).

**Gaps**
- Broad hardware testing and production app distribution outstanding.
- Remote signer support for mobile not yet built.

---

*Last updated: 2025‑09‑20*
