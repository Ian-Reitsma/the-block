# Project Progress Snapshot
> **Review (2025-10-02):** Flagged CLI parser migration gap and refreshed dependency readiness summary.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-29).

This document tracks high‑fidelity progress across The‑Block's major work streams.  Each subsection lists the current completion estimate, supporting evidence with canonical file or module references, and the remaining gaps.  Percentages are rough, *engineer-reported* gauges meant to guide prioritization rather than marketing claims.

Mainnet readiness currently measures **98.3/100** with vision completion **93.3/100**. Subsidy accounting now lives solely in the unified CT ledger; see `docs/system_changes.md` for migration notes. The standalone `governance` crate mirrors the node state machine for CLI/SDK use, the compute marketplace enforces lane-aware batching with fairness deadlines, starvation telemetry, and per-lane persistence, the mobile gateway cache persists encrypted responses with TTL hygiene plus CLI/RPC/telemetry visibility, wallet binaries share the crypto suite’s first-party Ed25519 backend with multisig signer telemetry, the RPC client clamps `TB_RPC_FAULT_RATE` while saturating exponential backoff, overlay discovery/uptime/persistence flow through the trait-based `p2p_overlay` crate with in-house and stub backends, the storage engine abstraction unifies RocksDB, sled, and memory providers via `crates/storage_engine`, the coding crate gates XOR parity and RLE compression fallbacks behind audited rollout policy while tagging storage telemetry and powering the bench-harness comparison mode, the gossip relay couples an LRU-backed dedup cache with adaptive fanout and partition tagging, the proof-rebate tracker persists receipts that land in coinbase assembly with explorer/CLI pagination, wrapper telemetry exports runtime/transport/storage/coding/codec/crypto metadata through both node metrics and the aggregator `/wrappers` endpoint, release provenance now hashes the vendored tree while recording dependency snapshots enforced by CI, CLI, and governance overrides, and the runtime-backed HTTP client now covers node/CLI surfaces while the aggregator and gateway servers still ride `axum`. The dependency-sovereignty pivot is documented in [`docs/pivot_dependency_strategy.md`](pivot_dependency_strategy.md) and reflected across every subsystem guide. Remaining focus areas: deliver treasury disbursement tooling, complete the in-house HTTP server rollout (aggregator + gateway), extend bridge/DEX docs with signer-set payloads and release-verifier guidance, wire compute-market SLA slashing dashboards atop the new matcher, continue WAN-scale QUIC chaos drills, polish multisig UX, and replace remaining clap/toml-based CLIs with `cli_core` plus the JSON codec.

\[
\text{multiplier}_x = \frac{\phi_x I_{\text{target}} S / 365}{U_x / \text{epoch\_secs}}
\]

clamped to ±15 % of the previous value. Base miner rewards decrease as the effective miner count rises following

\[
R_0(N) = \frac{R_{\max}}{1 + e^{\xi (N - N^\star)}}
\]

with hysteresis `ΔN ≈ √N*` to blunt flash joins. Full derivations live in [`docs/economics.md`](economics.md). The canonical roadmap with near‑term tasks lives in [`docs/roadmap.md`](roadmap.md).

## Dependency posture

- **Policy source**: [`config/dependency_policies.toml`](../config/dependency_policies.toml) enforces a depth limit of 3, assigns risk tiers, and blocks AGPL/SSPL transitively.  The registry snapshot is materialised via `cargo run -p dependency_registry -- --check config/dependency_policies.toml` and stored at [`docs/dependency_inventory.json`](dependency_inventory.json).
- **Current inventory** *(generated at `2025-09-30T12:54:59.759213+00:00`)*: 7 strategic crates, 7 replaceable crates, and 841 unclassified dependencies in the resolved workspace DAG.
- **Outstanding drift**: 210 dependencies currently breach policy depth and are tracked in [`docs/dependency_inventory.violations.json`](dependency_inventory.violations.json).  CI now uploads the generated registry and policy violations for each pull request and posts a summary so reviewers can block regressions quickly.
- **Next refresh**: Run `./scripts/dependency_snapshot.sh` on **2025-10-01** after the explorer `codec` link lands to capture the updated workspace DAG and refresh these metrics.

## 1. Consensus & Core Execution — 93.6 %

**Evidence**
- Hybrid PoW/PoS chain: `node/src/consensus/pow.rs` embeds PoS checkpoints and `node/src/consensus/fork_choice.rs` prefers finalized chains.
- Kalman-weighted multi-window difficulty retune with `retune_hint` metrics in `node/src/consensus/difficulty_retune.rs` and `docs/difficulty.md`.
- Rollback checkpoints and partition recovery hooks in `node/src/consensus/fork_choice.rs` and `node/tests/partition_recovery.rs`.
- EIP‑1559 base fee tracker: `node/src/fees.rs` adjusts per block and `node/tests/base_fee.rs` verifies target fullness tracking.
- Adversarial rollback tests in `node/tests/finality_rollback.rs` assert ledger state after competing forks.
- Coinbase assembly applies proof rebates via `node/src/blockchain/process.rs::apply_rebates`, with restart/reorg coverage in `node/tests/light_client_rebates.rs`.
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

## 2. Networking & Gossip — 98.4 %

**Evidence**
- Runtime-owned TCP/UDP reactor now backs the node RPC client/server plumbing (`crates/runtime/src/net.rs`, `node/src/rpc/client.rs`), while gateway and metrics-aggregator endpoints still rely on `hyper`/`axum` pending their migration. Buffered IO helpers live in `crates/runtime/src/io.rs` with integration coverage in `crates/runtime/tests/net.rs`.
- Deterministic gossip with partition tests: `node/tests/net_gossip.rs` and docs in `docs/networking.md`.
- QUIC transport with mutual-TLS certificate rotation, cached diagnostics, TCP fallback, provider introspection, and mixed-transport fanout; integration covered in `node/tests/net_quic.rs`, `crates/transport/src/lib.rs`, `crates/transport/src/quinn_backend.rs`, `crates/transport/src/s2n_backend.rs`, and `docs/quic.md`, with telemetry via `quic_cert_rotation_total`, `quic_provider_connect_total{provider}`, and per-peer `quic_retransmit_total`/`quic_handshake_fail_total` counters.
- Overlay abstraction via `crates/p2p_overlay` with in-house and stub backends, configuration toggles, CLI overrides, JSON-backed persistence, integration tests exercising the in-house backend, telemetry gauges (`overlay_backend_active`, `overlay_peer_total`, persisted counts) exposed through `node/src/telemetry.rs`, `cli/src/net.rs`, and `node/src/rpc/peer.rs`, and base58-check peer IDs wired through CLI/RPC/gateway diagnostics, including the latest fanout set surfaced in `net gossip_status`.
- Provider metadata and certificate validation now flow through `p2p::handshake`, which consumes the registry capability enums, persists provider IDs for CLI/RPC output, and loads retry/certificate policies from `config/quic.toml`.
- `net.quic_stats` RPC and `blockctl net quic stats` expose cached latency,
  retransmit, and endpoint reuse data with per-peer failure metrics for operators.
- LRU-backed duplicate suppression, adaptive fanout, and shard-aware persistence documented in `docs/gossip.md` and implemented in `node/src/gossip/relay.rs` with configurable TTL/fanout stored in `config/gossip.toml`.
  - `net gossip-status` CLI / `net.gossip_status` RPC expose live TTL, cache, fanout, partition tags, and persisted shard peer sets for operators.
  - Peer identifier fuzzing prevents malformed IDs from crashing DHT routing (`net/fuzz/peer_id.rs`).
  - Manual DHT recovery runbook (`docs/networking.md#dht-recovery`).
  - Peer database and chunk cache persist across restarts with configurable paths (`node/src/net/peer.rs` via `TB_PEER_DB_PATH` and `TB_CHUNK_DB_PATH`); `TB_PEER_SEED` fixes shuffle order for reproducible bootstraps.
  - ASN-aware A* routing oracle (`node/src/net/a_star.rs`) chooses k cheapest paths per shard and feeds compute-placement SLAs.
  - SIMD Xor8 rate-limit filter with AVX2/NEON dispatch (`node/src/web/rate_limit.rs`, `docs/benchmarks.md`) handles 1 M rps bursts.
  - Jittered JSON‑RPC client with exponential backoff (`node/src/rpc/client.rs`) prevents thundering-herd reconnect storms.
  - Gateway DNS publishing and policy retrieval logged in `docs/gateway_dns.md` and implemented in `node/src/gateway/dns.rs`.
    - Per-peer rate-limit telemetry and reputation tracking via `net.peer_stats` RPC and `net stats` CLI, capped by `max_peer_metrics`, with dashboards ingesting `GOSSIP_PEER_FAILURE_TOTAL` and `GOSSIP_LATENCY_BUCKETS`.
     - Partition watch detects split-brain conditions and stamps gossip with markers (`node/src/net/partition_watch.rs`, `node/src/gossip/relay.rs`).
     - Cluster-wide metrics pushed to the `metrics-aggregator` crate for fleet visibility.
    - Shard-aware peer maps and gossip routing limit block broadcasts to interested shards (`node/src/gossip/relay.rs`).
    - Uptime-based fee rebates tracked in `node/src/net/uptime.rs` with `peer.rebate_status` RPC (`docs/fee_rebates.md`).

**Gaps**
- Large-scale WAN chaos experiments remain open; cross-provider failover drills still pending.
- Bootstrap peer churn analysis missing.
    - Overlay soak tests need long-lived fault injection, and the dependency registry now focuses on automating storage migration drills plus the upcoming dependency fault simulation harness to certify fallbacks.

## 3. Governance & Subsidy Economy — 96.4 %

**Evidence**
- Subsidy multiplier proposals surfaced via `node/src/rpc/governance.rs` and web UI (`tools/gov-ui`).
- Shared `governance` crate re-exports bicameral voting, sled-backed `GovStore`, proposal DAG validation, Kalman retune helpers, and release workflows for CLI/SDK consumers (`governance/src/lib.rs` and examples).
- Push notifications on subsidy balance changes (`wallet` tooling).
- Explorer indexes settlement receipts with query endpoints (`explorer/src/lib.rs`).
- Risk-sensitive Kalman–LQG governor with variance-aware smoothing (`node/src/governance/kalman.rs`, `node/src/governance/variance.rs`).
- Laplace-noised multiplier releases and miner-count logistic hysteresis (`node/src/governance/params.rs`, `pow/src/reward.rs`).
- Emergency kill switch `kill_switch_subsidy_reduction` with telemetry counters (`node/src/governance/params.rs`, `docs/monitoring.md`).
- Subsidy accounting is unified in the CT ledger with migration documented in `docs/system_changes.md`.
- Proof-rebate tracker persists per-relayer receipts with governance rate clamps and coinbase integration (`node/src/light_client/proof_tracker.rs`, `node/src/blockchain/process.rs`, `docs/light_client_incentives.md`).
- Multi-signature release approvals persist signer sets and thresholds (`node/src/governance/release.rs`), gated fetch/install flows (`node/src/update.rs`, `cli/src/gov.rs`), and explorer/CLI timelines (`explorer/src/release_view.rs`, `contract explorer release-history`).
- Telemetry counters `release_quorum_fail_total` and `release_installs_total` expose quorum health and rollout adoption for dashboards.
- Fee-floor window and percentile parameters (`node/src/governance/params.rs`) stream through `GovStore` history with rollback support (`node/src/governance/store.rs`), governance CLI updates (`cli/src/gov.rs`), explorer timelines (`explorer/src/lib.rs`), and regression coverage (`governance/tests/mempool_params.rs`).
- DID revocations share the same `GovStore` history and prevent further anchors until governance clears the entry; the history is available to explorer and wallet tooling so revocation state can be surfaced alongside DID records (`node/src/governance/store.rs`, `node/src/identity/did.rs`, `docs/identity.md`).
- Simulations `sim/release_signers.rs` and `sim/lagging_release.rs` model signer churn and staggered downloads to validate quorum durability and rollback safeguards before production deployment.
- One‑dial multiplier formula retunes β/γ/κ/λ per epoch using realised utilisation `U_x`, clamped to ±15 % and doubled when `U_x` → 0; see `docs/economics.md`.
- Demand gauges `industrial_backlog` and `industrial_utilization` feed
    `Block::industrial_subsidies()` and surface via `inflation.params` and
    `compute_market.stats`.
- `pct_ct` tracks CT fee routing; production lanes pin the selector to 100 while `reserve_pending` debits balances before coinbase accumulation (`docs/fees.md`).
- Logistic base reward `R_0(N) = R_max / (1 + e^{ξ (N - N^*)})` with hysteresis `ΔN ≈ √N*` dampens miner churn and is implemented in `pow/src/reward.rs`.
 - Kalman filter weights for difficulty retune configurable via governance parameters (`node/src/governance/params.rs`).

**Gaps**
- Publish explorer timelines for proposal windows and upcoming treasury disbursements emitted by the CLI/governance crate.
- No on‑chain treasury or proposal dependency system.
- Governance rollback simulation incomplete.

## 4. Storage & Free‑Read Hosting — 93.8 %

**Evidence**
- Read acknowledgement batching and audit flow documented in `docs/read_receipts.md` and `docs/storage_pipeline.md`.
- Disk‑full metrics and recovery tests (`node/tests/storage_disk_full.rs`).
- Gateway HTTP parsing fuzz harness (`gateway/fuzz`).
- In-house LT fountain overlay for BLE repair (`node/src/storage/repair.rs`, `docs/storage/repair.md`, `node/tests/fountain_repair.rs`).
- Thread-safe `ReadStats` telemetry and analytics RPC (`node/src/telemetry.rs`, `node/tests/analytics.rs`).
- WAL-backed `SimpleDb` design in `docs/simple_db.md` underpins DNS cache, chunk gossip, and DEX storage.
- Unified `storage_engine` crate wraps RocksDB, sled, and in-memory engines with shared traits, concurrency-safe batches, crash-tested temp dirs, and configuration-driven overrides (`crates/storage_engine`, `node/src/simple_db/mod.rs`).
- `crates/coding` fronts encryption, erasure, fountain, and compression primitives; XOR parity and RLE fallback compressors respect `config/storage.toml` rollout gates, emit coder/compressor labels on storage latency and failure metrics, log `algorithm_limited` repair skips, and feed the `bench-harness compare-coders` mode for performance baselining (`crates/coding/src`, `node/src/storage/settings.rs`, `tools/bench-harness/src/main.rs`).
- Base64 snapshots stage through `NamedTempFile::persist` plus `sync_all`, with legacy dumps removed only after durable rename (`node/src/simple_db/memory.rs`, `node/tests/simple_db/memory_tests.rs`).
- Rent escrow metrics (`rent_escrow_locked_ct_total`, etc.) exposed in `docs/monitoring.md` with alert thresholds.
- Metrics aggregator ingestion still runs on `axum`/`tokio`; only the outbound log correlation calls use the shared `httpd::HttpClient` (`metrics-aggregator/src/lib.rs`). Runtime-backed ingestion and retention rework remain outstanding.
- Mobile gateway cache persists ChaCha20-Poly1305–encrypted responses and queued transactions to sled with TTL sweeping, eviction guardrails, telemetry counters, CLI `mobile-cache status|flush` commands, RPC inspection endpoints, and invalidation hooks (`node/src/gateway/mobile_cache.rs`, `node/src/rpc/gateway.rs`, `cli/src/gateway.rs`, `docs/mobile_gateway.md`). A min-heap of expirations drives sweep cadence, persistence snapshots reconstruct queues on restart, encryption keys derive from `TB_MOBILE_CACHE_KEY_HEX`/`TB_NODE_KEY_HEX`, and status responses expose per-entry age/expiry plus queue bytes so operators can tune TTL windows and capacity.
- Reputation-weighted Lagrange allocation and proof-of-retrievability challenges secure storage contracts (`node/src/gateway/storage_alloc.rs`, `storage/src/contract.rs`).

**Gaps**
- Incentive‑backed DHT storage marketplace still conceptual.
- Offline escrow reconciliation absent.

## 5. Smart‑Contract VM & UTXO/PoW — 87.5 %

**Evidence**
- Persistent `ContractStore` with CLI deploy/call flows (`state/src/contracts`, `cli/src/main.rs`).
- ABI generation from opcode enum (`node/src/vm/opcodes.rs`).
- State survives restarts (`node/tests/vm.rs::state_persists_across_restarts`).
- Planned dynamic gas fee market (`node/src/fees.rs` roadmap) anchors eventual EIP-1559 adaptation.
- Deterministic WASM runtime with fuel-based metering and ABI helpers (`node/src/vm/wasm/mod.rs`, `node/src/vm/wasm/gas.rs`).
- Interactive debugger and trace export (`node/src/vm/debugger.rs`, `docs/vm_debugging.md`).
- VM trace WebSocket streaming now rides the in-house runtime sockets (`node/src/rpc/vm_trace.rs`, `crates/runtime/src/net.rs`), keeping debugger tooling aligned with the dependency-sovereignty goals.

**Gaps**
- Instruction set remains minimal; no formal VM spec or audits.
- Developer SDK and security tooling pending.

## 6. Compute Marketplace & CBM — 95.8 %

**Evidence**
- Deterministic GPU/CPU hash runners (`node/src/compute_market/workloads`).
- Compute marketplace RPC endpoints still run through the bespoke parser backed by `runtime::net::TcpListener` in `node/src/rpc/mod.rs`; the `crates/httpd` router remains unused on the server side, so the dependency risk persists until that migration lands (`node/tests/compute_market_rpc_errors.rs`).
- `compute.job_cancel` RPC releases resources and refunds bonds (`node/src/rpc/compute_market.rs`).
- Capability-aware scheduler matches CPU/GPU workloads, weights offers by provider reputation, and handles cancellations (`node/src/compute_market/scheduler.rs`).
- Price board persistence with metrics (`docs/compute_market.md`).
- Lane-aware matching enforces per-`FeeLane` queues, fairness windows, and starvation timers, throttles via `TB_COMPUTE_MATCH_BATCH`, records `MATCH_LOOP_LATENCY_SECONDS{lane}` histograms, persists receipts with lane tags for replay safety, and surfaces queue depths/capacity guardrails through RPC/CLI (`node/src/compute_market/matcher.rs`, `node/tests/compute_matcher.rs`, `node/src/rpc/compute_market.rs`, `cli/src/compute.rs`). The matcher rotates lanes until a batch quota or fairness deadline triggers, rejects staged seeds that exceed capacity, emits structured starvation warnings with job IDs/ages, and annotates `compute_market.stats` with per-lane wait durations for operators.
- Settlement persists CT balances, audit logs, activation metadata, and Merkle roots in a RocksDB-backed store with RPC/CLI/explorer surfacing (`node/src/compute_market/settlement.rs`, `node/tests/compute_settlement.rs`, `docs/compute_market.md`, `docs/settlement_audit.md`, `explorer/src/compute_view.rs`). The ledger emits telemetry (`SETTLE_APPLIED_TOTAL`, `SETTLE_FAILED_TOTAL{reason}`, `SETTLE_MODE_CHANGE_TOTAL{state}`, `SLASHING_BURN_CT_TOTAL`, `COMPUTE_SLA_VIOLATIONS_TOTAL{provider}`) and exposes `compute_market.provider_balances`, `compute_market.audit`, and `compute_market.recent_roots` RPCs for automated reconciliation.
- `Settlement::shutdown` persists any pending ledger deltas and flushes RocksDB handles before teardown so test harnesses (and unplanned exits) leave behind consistent CT balances and Merkle roots for replay.
- Admission enforces dynamic fee floors with per-sender slot caps, eviction audit trails, explorer charts, and `mempool.stats` exposure (`node/src/mempool/admission.rs`, `node/src/mempool/scoring.rs`, `docs/mempool_qos.md`, `node/tests/mempool_eviction.rs`). Governance parameters for the floor window and percentile stream through telemetry (`fee_floor_window_changed_total`, `fee_floor_warning_total`, `fee_floor_override_total`) and wallet guidance.
- `FeeFloor::new(size, percentile)` now requires explicit percentile inputs in tests and CLI paths, aligning mempool QoS regressions with governance-configured sampling windows (`node/src/mempool/scoring.rs`, `node/tests/mempool_qos.rs`).
- Economic simulator outputs KPIs to CSV (`sim/src`).
- Durable courier receipts with exponential backoff documented in `docs/compute_market_courier.md` and implemented in `node/src/compute_market/courier.rs`.
- Groth16/Plonk SNARK verification for compute receipts (`node/src/compute_market/snark.rs`).
- Policy pins `fee_pct_ct` to CT-only payouts for production lanes while retaining selector compatibility in tests (`node/src/compute_market/mod.rs`, `docs/compute_market.md`).

**Gaps**
- Escrowed payments and automated SLA enforcement remain rudimentary; deadline tracking and slashing heuristics are staged but not yet active.

## 7. Trust Lines & DEX — 85.9 %

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

## 8. Wallets, Light Clients & KYC — 96.6 %

**Evidence**
- CLI + hardware wallet support (`crates/wallet`).
- Remote signer workflows (`crates/wallet/src/remote_signer.rs`, `docs/wallets.md`).
- Remote signer HTTP calls now rely on the blocking wrapper in `crates/httpd`, eliminating external clients while keeping retry/backoff semantics intact (`crates/wallet/src/remote_signer.rs`, `crates/httpd/src/blocking.rs`).
- Mobile light client with push notification hooks (`examples/mobile`, `docs/mobile_light_client.md`).
- Light-client synchronization and header verification documented in `docs/light_client.md`.
- Device status probes integrate Android/iOS power and connectivity hints, cache asynchronous readings with graceful degradation, emit `the_block_light_client_device_status{field,freshness}` telemetry, persist overrides in `~/.the_block/light_client.toml`, surface CLI/RPC gating messages, and embed annotated snapshots in compressed log uploads (`crates/light-client`, `cli/src/light_client.rs`, `docs/light_client.md`, `docs/mobile_light_client.md`).
- Real-time state streaming over WebSockets with hybrid (lz77-rle) snapshots (`docs/light_client_stream.md`, `node/src/rpc/state_stream.rs`).
- Optional KYC provider wiring (`docs/kyc.md`).
- Session-key issuance and meta-transaction tooling (`crypto/src/session.rs`, `cli/src/wallet.rs`, `docs/account_abstraction.md`).
- Telemetry `session_key_issued_total`/`session_key_expired_total` and simulator churn knob (`sim/src/lib.rs`).
- Release fetch/install tooling verifies provenance, records timestamps, and exposes explorer/CLI history for operator audits (`node/src/update.rs`, `cli/src/gov.rs`, `explorer/src/release_view.rs`).
- Wallet send flow caches fee-floor lookups, emits localized warnings with auto-bump or `--force` overrides, streams telemetry events back to the node, and exposes JSON mode for automation (`cli/src/wallet.rs`, `docs/mempool_qos.md`).
- Unified crypto suite Ed25519 signature handling (first-party backend) ensures remote signer payloads, CLI staking flows, and explorer attestations all share compatible types while forwarding multisig signer arrays and escrow hash algorithms (`crates/wallet`, `node/src/bin/wallet.rs`, `tests/remote_signer_multisig.rs`).
- Remote signer metrics (`remote_signer_request_total`, `remote_signer_success_total`, `remote_signer_error_total{reason}`) integrate with wallet QoS counters so dashboards highlight signer outages alongside fee-floor overrides (`crates/wallet/src/remote_signer.rs`, `docs/monitoring.md`).
- Light-client rebate history and leaderboards exposed via RPC/CLI/explorer (`node/src/rpc/light.rs`, `cli/src/light_client.rs`, `explorer/src/light_client.rs`, `docs/light_client_incentives.md`).

**Gaps**
- Polish multisig UX (batched signer discovery, richer operator prompts) before tagging the next CLI release.
- Surface multisig signer history in explorer/CLI output for auditability.
- Production‑grade mobile apps not yet shipped.

## 9. Bridges & Cross‑Chain Routing — 81.9 %

**Evidence**
- Per-asset bridge channels with relayer sets, pending withdrawals, and bond ledgers persisted via `SimpleDb` (`node/src/bridge/mod.rs`).
- Multi-signature quorum enforcement and governance authorization hooks in `bridge.verify_deposit` and `governance::ensure_release_authorized`, covered by integration tests `node/tests/bridge.rs` and adversarial suites `bridges/tests/adversarial.rs`.
- Challenge windows and slashing logic (`bridge.challenge_withdrawal`, `bridges/src/relayer.rs`) debit collateral and emit telemetry `BRIDGE_CHALLENGES_TOTAL`/`BRIDGE_SLASHES_TOTAL`.
- Partition markers propagate through deposit events and withdrawal routing so relayers avoid isolated shards (`node/src/net/partition_watch.rs`, `docs/bridges.md`).
- CLI/RPC surfaces for quorum composition, pending withdrawals, history, and slash logs (`cli/src/bridge.rs`, `node/src/rpc/bridge.rs`).
- Bridge RPC endpoints continue to rely on the bespoke JSON-RPC loop in `node/src/rpc/mod.rs`; the planned `crates/httpd` server integration has not shipped yet, so quorum tooling still depends on the legacy routing until that swap completes.

**Gaps**
- Multi-asset wrapping, external settlement proofs, and long-horizon dispute audits remain.

## 10. Monitoring, Debugging & Profiling — 95.8 %

**Evidence**
  - Prometheus exporter with extensive counters (`node/src/telemetry.rs`).
  - Service badge tracker exports uptime metrics and RPC status (`node/src/service_badge.rs`, `node/tests/service_badge.rs`). See `docs/service_badge.md`.
  - Monitoring stack via `make monitor` and docs in `docs/monitoring/README.md`.
    - Cluster metrics aggregation with disk-backed retention (`metrics-aggregator` crate).
    - Aggregator ingestion still depends on `hyper`/`axum`; runtime-backed archive streaming is pending. Outbound correlations now share the node’s HTTP client (`metrics-aggregator/src/lib.rs`).
    - Metrics-to-logs correlation links Prometheus anomalies to targeted log dumps and exposes `log_correlation_fail_total` for missed lookups (`metrics-aggregator/src/lib.rs`, `node/src/rpc/logs.rs`, `cli/src/logs.rs`).
    - VM trace counters and partition dashboards (`node/src/telemetry.rs`, `monitoring/templates/partition.json`).
    - Settlement audit CI job (`.github/workflows/ci.yml`).
    - Fee-floor policy changes and wallet overrides surface via `fee_floor_window_changed_total`, `fee_floor_warning_total`, and `fee_floor_override_total`, while DID anchors increment `did_anchor_total` for explorer dashboards (`node/src/telemetry.rs`, `monitoring/metrics.json`, `docs/mempool_qos.md`, `docs/identity.md`).
    - Per-lane compute matcher counters (`matches_total{lane}`), latency histograms (`match_loop_latency_seconds{lane}`), starvation warnings, and mobile cache metrics (`mobile_cache_hit_total`, `mobile_cache_stale_total`, `mobile_cache_entry_bytes`, `mobile_cache_queue_bytes`, `mobile_tx_queue_depth`) feed dashboards alongside the `the_block_light_client_device_status{field,freshness}` gauge for background sync diagnostics (`node/src/telemetry.rs`, `docs/telemetry.md`, `docs/mobile_gateway.md`, `docs/light_client.md`).
    - Storage ingest and repair metrics carry `erasure`/`compression` labels so fallback rollouts can be tracked in Grafana, and repair skips log `algorithm_limited` contexts for incident reviews (`node/src/telemetry.rs`, `docs/monitoring.md`, `docs/storage_erasure.md`).
    - Wrapper telemetry exports runtime/transport/overlay/storage/coding/codec/crypto metadata via `runtime_backend_info`, `transport_provider_connect_total{provider}`, `codec_serialize_fail_total{profile}`, and `crypto_suite_signature_fail_total{backend}`. The `metrics-aggregator` exposes a `/wrappers` endpoint for fleet summaries, Grafana dashboards render backend selections/failure rates, and `contract-cli system dependencies` fetches on-demand snapshots for operators (`node/src/telemetry.rs`, `metrics-aggregator/src/lib.rs`, `monitoring/metrics.json`, `monitoring/grafana/*.json`, `cli/src/system.rs`).
    - Incremental log indexer resumes from offsets, rotates encryption keys, streams over WebSocket, and exposes REST filters (`tools/log_indexer.rs`, `docs/logging.md`).

**Gaps**
- Bridge and VM metrics are sparse.
- Automated anomaly detection not in place.

## 11. Identity & Explorer — 83.4 %

**Evidence**
- DID registry persists anchors with replay protection, governance revocation checks, and optional provenance attestations (`node/src/identity/did.rs`, `state/src/did.rs`).
- Light-client commands anchor and resolve DIDs with remote signer support, sign-only payload export, and JSON output for automation (`cli/src/light_client.rs`, `examples/did.json`).
- Explorer ingests DID updates into `did_records`, serves `/dids`, `/identity/dids/:address`, and anchor-rate metrics for dashboards (`explorer/src/did_view.rs`, `explorer/src/main.rs`).
- Explorer caches DID lookups in-memory to avoid redundant RocksDB reads and drives anchor-rate dashboards from `/dids/metrics/anchor_rate` (`explorer/src/did_view.rs`, `explorer/src/main.rs`).
- Governance history captures fee-floor and DID revocations for auditing alongside wallet telemetry (`node/src/governance/store.rs`, `docs/identity.md`).

**Gaps**
- Revocation alerting and recovery runbooks need explorer/CLI integration.
- Mobile wallet identity UX and bulk export tooling remain outstanding.

## 12. Economic Simulation & Formal Verification — 43.0 %

**Evidence**
- Simulation scenarios for inflation/demand/backlog (`sim/src`).
- F* scaffolding for consensus proofs (`formal/` installers and docs).
- Scenario library exports KPIs to CSV.

**Gaps**
- Formal proofs beyond scaffolding missing.
- Scenario coverage still thin.

## 13. Mobile UX & Contribution Metrics — 73.2 %

**Evidence**
- Background sync respecting battery/network constraints with platform-specific probes, async caching, CLI/RPC gating messages, and persisted overrides (`docs/light_client.md`, `docs/mobile_light_client.md`, `cli/src/light_client.rs`). Device snapshots capture freshness (`fresh|cached|fallback`) labels, stream to `the_block_light_client_device_status`, embed into compressed log uploads, and expose CLI toggles for charging/Wi‑Fi overrides stored in `~/.the_block/light_client.toml`.
- Contribution metrics and optional KYC in mobile example (`examples/mobile`).
- Push notifications for subsidy events (wallet tooling) plus encrypted mobile cache persistence with TTL hygiene, size guardrails, and CLI flush hooks for reliable offline recovery (`node/src/gateway/mobile_cache.rs`, `docs/mobile_gateway.md`).

**Gaps**
- Broad hardware testing and production app distribution outstanding.
- Remote signer support for mobile not yet built.

---

*Last updated: 2025‑09‑29*
