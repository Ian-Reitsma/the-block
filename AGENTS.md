# AGENTS.md — **The‑Block** Developer Handbook

Quick Index
- Vision & Strategy: see §16
- Agent Playbooks: see §17
- Strategic Pillars: see §18
- Monitoring Stack: see `docs/monitoring.md` and `make monitor`
- Status & Roadmap: see `docs/roadmap.md`
- Progress Snapshot: see `docs/progress.md` for subsystem status and gaps
- Networking, per-peer telemetry, & DHT recovery: see `docs/networking.md`
- QUIC handshake & fallback rules: see `docs/quic.md`
- Economic formulas: see `docs/economics.md`
- Blob root scheduling: see `docs/blob_chain.md`
- Macro-block checkpoints: see `docs/macro_block.md`
- Law-enforcement portal & canary runbook: see `docs/le_portal.md`
- Range-boost queue semantics: see `docs/range_boost.md`
- Read acknowledgement batching and audit workflow: see `docs/read_receipts.md`
- RocksDB layout, crash recovery, and simulation replay: see `state/README.md`
- Parallel execution and transaction scheduling: see `docs/scheduler.md`
- PoH tick generator: see `docs/poh.md`
- Commit–reveal scheme: see `docs/commit_reveal.md`
- Service badge tracker: see `docs/service_badge.md`
- Fee market reference: see `docs/fees.md`
- Network fee rebates: see `docs/fee_rebates.md`
- Transaction lifecycle and fee lanes: see `docs/transaction_lifecycle.md`
- Compute-market courier retry logic: see `docs/compute_market_courier.md`
- Compute-market admission quotas: see `docs/compute_market.md`
- Compute-unit calibration: see `docs/compute_market.md`
- Compute-market SNARK receipts: see `docs/compute_snarks.md`
- Multi-hop trust-line routing: see `docs/dex.md`
- DEX escrow and partial-payment proofs: see `docs/dex.md`
- AMM pools and liquidity mining: see `docs/dex_amm.md`
- Gateway DNS publishing and policy records (`.block` TLD or externally verified): see `docs/gateway_dns.md`
- Gossip relay dedup and adaptive fanout: see `docs/gossip.md`
- P2P handshake and capability negotiation: see `docs/p2p_protocol.md`
- Light-client synchronization and security model: see `docs/light_client.md`
- Light-client state streaming: see `docs/light_client_stream.md`
- Bridge light-client verification: see `docs/bridges.md`
- Jurisdiction policy packs and LE logging: see `docs/jurisdiction.md`
- Probe CLI and metrics: see `docs/probe.md`
- Operator QUIC configuration and difficulty monitoring: see `docs/operators/run_a_node.md`
- Python demo walkthrough: see `docs/demo.md`
- Telemetry summaries and histograms: see `docs/telemetry.md`
- Simulation framework and replay semantics: see `docs/simulation_framework.md`
- Wallet staking lifecycle: see `docs/wallets.md`
- Remote signer workflows: see `docs/wallets.md`
- Storage erasure coding and reconstruction: see `docs/storage_erasure.md`
- Storage market incentives and proofs-of-retrievability: see `docs/storage_market.md`
- KYC provider workflow: see `docs/kyc.md`
- A* latency routing: see `docs/net_a_star.md`
- Mempool architecture and tuning: see `docs/mempool.md`
- Hash layout & genesis seeding: see `docs/hashlayout.md`
- State pruning and RocksDB compaction: see `docs/state_pruning.md`
- Cross-platform deployment methods: see `docs/deployment_guide.md`
- Build provenance and attestation: see `docs/provenance.md`

> **Read this once, then work as if you wrote it.**  Every expectation, switch, flag, and edge‑case is documented here.  If something is unclear, the failure is in this file—open an issue and patch the spec *before* you patch the code.

Mainnet readiness sits at **~99.6/100** with vision completion **~86.9/100**. Subsidy accounting is unified around the CT subsidy categories (`STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT`) with ledger snapshots shared across the node, governance crate, CLI, and explorer.
Recent additions now include multi-signature release approvals with explorer and CLI support, attested binary fetch with automated rollback, QUIC mutual-TLS rotation plus diagnostics and chaos tooling, mempool QoS slot accounting, and end-to-end metrics-to-log correlation surfaced through the aggregator and dashboards. Governance now tracks fee-floor policy history with rollback support, wallet flows surface localized floor warnings with telemetry hooks and JSON output, DID anchoring runs through on-chain registry storage with explorer timelines, and light-client commands handle sign-only payloads as well as remote provenance attestations. Macro-block checkpointing, per-shard state roots, SNARK-verified compute receipts, real-time light-client state streaming, Lagrange-coded storage allocation with proof-of-retrievability, network fee rebates, deterministic WASM execution with a stateful debugger, build provenance attestation, session-key abstraction, Kalman difficulty retune, and network partition recovery continue to extend the cluster-wide `metrics-aggregator` and graceful `compute.job_cancel` RPC.

**Latest highlights:**
- Governance, SDKs, and the CLI now consume the shared `governance` crate with sled-backed `GovStore`, proposal DAG validation, Kalman retune helpers, and release quorum enforcement, keeping every integration on the node’s canonical state machine.
- Wallet binaries continue to ship on `ed25519-dalek 2.2.x`, propagate multisig signer sets, escrow hash algorithms, and remote signer telemetry, and surface localized fee-floor coaching with JSON automation hooks for dashboards.
- Compute-market settlement writes CT/IT movements to a RocksDB ledger that tracks activation metadata, audit exports, and recent Merkle roots exposed through RPC, CLI, and explorer views for cross-restart reconciliation; `Settlement::shutdown` persists pending entries and flushes RocksDB so operators can assert clean teardown in integration harnesses.
- RPC clients clamp `TB_RPC_FAULT_RATE`, saturate exponential backoff after the 31st attempt, guard environment overrides with scoped restorers, and expose regression coverage so operators can trust bounded retry behaviour during incidents.
- `SimpleDb` snapshot rewrites stage data through fsync’d temporary files, atomically rename into place, and retain legacy dumps until the new image lands, eliminating crash-window data loss while keeping legacy reopen logic intact.
- Node CLI binaries honour telemetry/gateway feature toggles, emitting explicit user-facing errors when unsupported flags are passed, recording jurisdiction languages in law-enforcement audit logs, and compiling via optional feature bundles (`full`, `wasm-metadata`, `sqlite-storage`) for memory-constrained tests.
- Light-client state streaming, DID anchoring, and explorer timelines now trace revocations and provenance attestations end-to-end with cached pagination so wallet, CLI, and dashboards agree on identity state.

**Outstanding focus areas:**
- Ship governance treasury disbursement tooling and explorer timelines before opening external treasury submissions.
- Harden compute-market SLA enforcement with deadline slashing, telemetry, and operator remediation guides.
- Continue WAN-scale QUIC chaos drills for relay fan-out while publishing mitigation recipes from the new telemetry traces.
- Finish multisig wallet UX polish (batched signer discovery, richer CLI prompts) so remote signers can run production workflows.
- Expand bridge and DEX documentation with signer-set payloads, explorer telemetry, and release-verifier guidance ahead of the next tag.

---

## Table of Contents

1. [Project Mission & Scope](#1-project-mission--scope)
2. [Repository Layout](#2-repository-layout)
3. [System Requirements](#3-system-requirements)
4. [Bootstrapping & Environment Setup](#4-bootstrapping--environment-setup)
5. [Build & Install Matrix](#5-build--install-matrix)
6. [Testing Strategy](#6-testing-strategy)
7. [Continuous Integration](#7-continuous-integration)
8. [Coding Standards](#8-coding-standards)
9. [Commit & PR Protocol](#9-commit--pr-protocol)
10. [Subsystem Specifications](#10-subsystem-specifications)
11. [Security & Cryptography](#11-security--cryptography)
12. [Persistence & State](#12-persistence--state)
13. [Troubleshooting Playbook](#13-troubleshooting-playbook)
14. [Glossary & References](#14-glossary--references)
15. [Outstanding Blockers & Directives](#15-outstanding-blockers--directives)
16. [Vision & Strategy](#16-vision--strategy)
17. [Agent Playbooks — Consolidated](#17-agent-playbooks--consolidated)

---

## 1 · Project Mission & Scope — Production-Grade Mandate

**The‑Block** is a *formally‑specified*, **Rust-first**, dual-token, proof‑of‑work + proof‑of‑service blockchain kernel destined for main-net deployment.
The repository owns exactly four responsibility domains:

| Domain        | In-Scope Artifacts                                                     | Out-of-Scope (must live in sibling repos) |
|---------------|------------------------------------------------------------------------|-------------------------------------------|
| **Consensus** | State-transition function; fork-choice; difficulty retarget; header layout; emission schedule. | Alternative L2s, roll-ups, canary forks. |
| **Serialization** | Canonical bincode config; cross-lang test-vectors; on-disk schema migration. | Non-canonical “pretty” formats (JSON, GraphQL, etc.). |
| **Cryptography** | Signature + hash primitives, domain separation, quantum-upgrade hooks. | Hardware wallet firmware, MPC key-ceremony code. |
| **Core Tooling** | CLI node, cold-storage wallet, DB snapshot scripts, deterministic replay harness. | Web explorer, mobile wallets, dApp SDKs. |

**Design pillars (now hardened for production)**

| Pillar                        | Enforcement Mechanism | Production KPI |
|-------------------------------|-----------------------|----------------|
| Determinism ⇢ Reproducibility | CI diff on block-by-block replay across x86_64 & AArch64 in release mode; byte-equality Rust ↔ Python serialization tests. | ≤ 1 byte divergence allowed over 10 k simulated blocks. |
| Memory- & Thread-Safety       | `#![forbid(unsafe_code)]`; FFI boundary capped at 2 % LOC; Miri & AddressSanitizer in nightly CI. | 0 undefined-behaviour findings in continuous fuzz. |
| Portability                   | Cross-compile matrix: Linux glibc & musl, macOS, Windows‑WSL; reproducible Docker images. | Successful `cargo test --release` on all targets per PR. |

### Economic Model — CT/IT Subsidy Engine

- Subsidy accounting now lives in the shared CT ledger. All
  operator rewards flow in liquid CT and are minted directly in the
  coinbase.
- Every block carries three subsidy fields: `STORAGE_SUB_CT`,
  `READ_SUB_CT`, and `COMPUTE_SUB_CT`.
- `industrial_backlog` and `industrial_utilization` gauges feed
  `Block::industrial_subsidies()`; these metrics surface the queued work and
  realised throughput that the subsidy governor uses when retuning
  multipliers.
- Per‑epoch utilisation `U_x` (bytes stored, bytes served, CPU ms, bytes
  out) feeds the "one‑dial" multiplier formula:

  \[
  \text{multiplier}_x =
    \frac{\phi_x I_{\text{target}} S / 365}{U_x / \text{epoch\_secs}}
  \]

  Adjustments are clamped to ±15 % of the prior value; near‑zero
  utilisation doubles the multiplier to keep incentives alive. Governance
  may hot‑patch all multipliers via `kill_switch_subsidy_reduction`.
- Base miner reward follows a logistic curve

  \[
  R_0(N) = \frac{R_{\max}}{1+e^{\xi (N-N^\star)}}
  \]

  with hysteresis `ΔN ≈ √N*` to damp flash joins/leaves.
- See `docs/economics.md` for full derivations and worked examples.

## 2 · Repository Layout

```
node/
  src/
    bin/
    compute_market/
    net/
    lib.rs
    ...
  tests/
  benches/
  .env.example
crates/
monitoring/
examples/governance/
examples/workloads/
fuzz/wal/
formal/
scripts/
  bootstrap.sh
  bootstrap.ps1
  requirements.txt
  requirements-lock.txt
  docker/
demo.py
docs/
  compute_market.md
  service_badge.md
  wal.md
  snapshots.md
  monitoring.md
  formal.md
  detailed_updates.md
AGENTS.md
```

Tests and benches live under `node/`. If your tree differs, run the repo re‑layout task in this file.

## 3 · System Requirements

- Rust 1.86+, `cargo-nextest`, `cargo-fuzz` (nightly), and `maturin` for Python bindings.
- Python 3.12.3 in a virtualenv; bootstrap creates `bin/python` shim and prepends `.venv/bin` to `PATH`.
- Node 18+ for the monitoring stack; `npm ci --prefix monitoring` must succeed when `monitoring/**` changes.
- On Linux, `patchelf` is required for wheel installs (bootstrap installs it automatically).
### Disclaimer → Production Readiness Statement

No longer a toy. The‑Block codebase targets production-grade deployment under real economic value.
Every commit is treated as if main-net launch were tomorrow: formal proofs, multi-arch CI, and external security audits are mandatory gates.
Proceed only if you understand that errors here translate directly into on-chain financial risk.

### Vision Snapshot

*The-Block* ultimately targets a civic-grade chain: a one-second base layer
that anchors notarized micro-shards, dual Consumer/Industrial tokens, and an
inflation-subsidy meter that rewards honest node work. Governance follows the
"service guarantees citizenship" maxim—badges earned by uptime grant one
vote per node, with shard-based districts to check capture. This repository is
the kernel of that architecture.

### Current Foundation

The codebase already ships a reproducible kernel with:

- dynamic difficulty retargeting and one-second block cadence,
- dual-token fee routing and decay-driven emissions,
- purge-loop infrastructure with telemetry counters and TTL/orphan sweeps,
- a minimal TCP gossip layer and JSON-RPC control surface,
- cross-language serialization tests and a Python demo.

### Long-Term Goals

Future milestones add durable storage, authenticated peer discovery,
micro-shard bundle roots, quantum-ready crypto, and the full
service-based governance stack. See §16 “Vision & Strategy”
for the complete blueprint embedded in this document.


## 16 · Vision & Strategy

The following section is the complete, up‑to‑date vision. It supersedes any earlier, partial “vision” notes elsewhere in the repository. Treat this as the single source of truth for mission, launch strategy, governance posture, and roadmap narrative.

# Agents Vision and Strategy

Service Guarantees Citizenship: A Civic-Scale Architecture for a One-Second L1, Notarized Micro‑Shards, and Contribution‑Weighted Governance

## Abstract
The‑Block is a production‑grade, people‑powered blockchain designed to make everyday digital life faster, cheaper, and more trustworthy while rewarding real service. A simple, auditable 1‑second L1 handles value and policy; sub‑second micro‑shards batch heavy AI/data into notarized roots per tick. Economics use two tradeable tokens—Consumer (BLOCKc) for the retail surface and Industrial (BLOCKi) for compute settlement—plus non‑transferable, expiring bill‑reducers for smooth UX. Governance binds rights to earned service via bicameral votes (Operators + Builders), quorum, and timelocks. The networking model extends beyond classic blockchains: nearby devices form a “people‑built internet” (LocalNet + Range Boost) where proximity and motion become infrastructure, coverage earns more where it’s scarce, and money maps to useful time and reach. Launch proceeds consumer‑first (single USDC pool), with Industrial lanes lighting once readiness trips.

## 1. Introduction & Current State
Public chains excel in different slices—monetary credibility (Bitcoin), programmability (Ethereum), low latency (Solana), payments (XRP)—but none marries auditability, sub‑second data, wide participation, and service‑tied rights. Our blueprint: keep L1 minimal and deterministic; push heavy work to shards; pay for accepted results; and let “service guarantee citizenship.”

Already in‑repo:
- 1‑second L1 kernel (Rust), difficulty retarget, mempool validation
- dual‑token model, decay‑based emissions, fee selectors
- purge loops (TTL/orphan) with telemetry
- minimal gossip + JSON‑RPC node
- cross‑language determinism tests, Python demo

## 2. System Overview
### 2.1 One‑Second L1 + Notarized Micro‑Shards
L1: value transfers, governance, shard‑root receipts; fixed 256‑bit header; canonical encoding. Shards: domain lanes (AI/media/storage/search) at 10–50 ms; emit one root/tick with quorum attestations; inner data stays user‑encrypted and content‑addressed.

### 2.2 Service Identity & Roles
Nodes attest uptime and verifiable work (bandwidth/storage/compute). Each epoch, percentile ranking assigns roles: target ~70% Consumer / ~30% Industrial; roles lock for the epoch with hysteresis to prevent flapping.

### 2.3 Economics: Two Tokens + Bill Reducers
- BLOCKc (Consumer): retail‑facing fees and spendable balance
- BLOCKi (Industrial): compute settlement and operator rewards
- Personal rebates/priority (non‑transferable, expiring) auto‑apply to your own bills; never tradable, never change market price. Equal pay per slice; fast rigs earn more/hour by completing more slices.

## 3. Governance: Constitution vs Rulebook
**Constitution (immutable):** hard caps and monotone emissions; 1‑second cadence; one‑badge‑one‑vote; quorum + timelocks; no mint‑to‑EOA; no backdoors.

**Rulebook (bounded):** CON/IND split (±10%/quarter); industrial share target (20–50%); rebate accrual/expiry windows; base‑fee escalator bounds; treasury streaming caps; shard counts/admission; jurisdiction modules.

**Process:** bicameral votes (Operators/Builders); snapshot voters at create; secret ballots; param changes next epoch after timelock; upgrades require supermajority + longer timelock + rollback window; emergencies only at catalog/app layer, auto‑expire, fully logged.

## 4. Rewards, Fees, Emissions
- Two pots per block (CON/IND); CON pays all active nodes by uptime; IND weights validated work. If industrial supply is scarce, nudge split within bounds (economics, not edicts).
- Emissions anchored to block height; publish curve and tests; first‑month issuance stays tame (≈0.01% per token). No variable caps; vest any pre‑TGE accrual by uptime/validated work.
- Reads free; writes burn personal rebates first, then coins (BLOCKi for shard roots, BLOCKc for L1).

## 5. Privacy & UX
- Vault + Personal AI: default‑private content with revocable capabilities; explainable citations (which items answered a query); content encrypted at source; chain notarizes proofs only.
- OS‑native SDKs (iOS/Android/macOS/Windows) expose Open/Save/Share/Grant/Revoke; phones act as secure controllers/light relays; hubs/routers carry background work.
- Live trust label: funds can’t be taken; rules can’t skip timelocks; sharing is visible and revocable.

## 6. People‑Built Internet
### LocalNet (Fast Road)
Nearby devices bond uplinks, cache, and relay for instant starts and low latency; paid relays; visible speed boost for video/downloads/games.

### Range Boost (Long Road)
Delay‑tolerant store‑and‑forward across BLE/Wi‑Fi Direct/ISM bands; optional $15–$40 “lighthouse” dongles for rural reach; coverage pays more per byte where scarce.

### Carry‑to‑Earn & Update Accelerator
Phones earn by carrying sealed bundles along commutes; settlement releases on delivery proofs. Neighborhood Update Accelerator serves big updates/patches from nearby seeds (content‑addressed, verified) for instant downloads.

### Hotspot Exchange
User‑shared, rate‑limited guest Wi‑Fi with one‑tap join; earn at home, spend anywhere; roaming without passwords/SIMs; wrapped traffic and rate caps for host safety.

## 7. Compute Marketplace
- Per‑slice pricing; sealed‑bid batch matches with tiny deposits; equal pay per slice type.
- Canary lanes (transcode, authenticity checks) at Industrial TGE to set anchors; expand to heavier jobs with caps.
- Shadow intents (stake‑backed) pre‑TGE show p25–p75 bands; convert escrow to BLOCKi at go‑live; start jobs; rebates begin as personal bill reducers.
- Operator guardrails: daily per‑node payout caps; UI break‑even/margin probes (power cost × hours/shard × watts).

## 8. Compute‑Backed Money (CBM) & Instant Apps
- CBM: daily redeem curves—X BLOCK buys Y seconds standard compute or Z MB delivered; protocol enforces redeemability with a minimal backstop from marketplace fees.
- Instant Apps: tap‑to‑use applets execute via nearby compute/caches and settle later; creators paid per use in CBM; users often pay zero if they contributed.

## 9. Launch Plan
- Consumer‑first TGE: seed $500 USDC : 1,000,000 BLOCKc (single USDC pool), time‑lock LP, 48h slow‑start; publish pool math and addresses.
- Marketplace preview: stake‑backed intents show bands without orders.
- Readiness trips Industrial (nodes, capacity, liquidity, vote sustained N days): arm 72h countdown; list USDC/BLOCKi; auto‑convert escrows; start canary lanes; subsidies act as rebates.
- Vesting & caps: any pre‑TGE accrual vests by uptime/validated work; cap total pre‑launch claims.

## 10. SDKs
- Provenance: sensor‑edge signing, proof bundles, content hash anchoring; explainable citations.

## 11. Security, Legal & Governance Posture <a id="11-security--cryptography"></a>
- End‑to‑end encryption; protocol sees pointers and hashed receipts only; no master keys.
- Law‑enforcement: metadata only; catalogs log delists; public transparency log + warrant canary.
- Jurisdiction modules: client/provider consume versioned regional packs (consent defaults, feature toggles); community‑voted; forks allowed.
- Non‑custodial core; KYC/AML handled by ramps; OFAC‑aware warnings in UIs.
- Founder exit: burn protocol admin keys; reproducible builds; move marks/domains to a standards non‑profit; bicameral governance; public irrevocability txs.

## 12. Dashboard & Metrics
- Home: BLOCKc/day (USD est.), 7‑day sparkline; readiness score & bottleneck; node mix; inflation; circulating supply.
- Marketplace: job cards w/ p25–p75, p_adj; est. duration on your device; break‑even/margin; refundable capacity stakes.
- Wallet/Swap: balances, recent tx; DEX swap (USDC↔BLOCKc); no fiat in‑app.
- Policy: emissions curve; live R(t,b); reserve inventory; jurisdiction pack hashes; transparency log.

## 13. Roadmap

Mainnet readiness: ~99.4/100 · Vision completion: ~85.6/100. Known blockers: stabilise telemetry-gated integration warnings, finish bridge/DEX signer-set documentation, polish multisig UX, and continue WAN-scale QUIC chaos drills. See [docs/roadmap.md](docs/roadmap.md) and [docs/progress.md](docs/progress.md) for evidence and upcoming milestones.

**Recent**

- Stake-weighted PoS finality with validator registration, bonding/unbonding, and slashing RPCs.
- Proof-of-History tick generator and Turbine-style gossip for deterministic propagation.
- Parallel execution engine with optional GPU hash workloads.
- Modular wallet framework with hardware signer support and CLI utilities.
- Cluster-wide `metrics-aggregator` service and graceful `compute.job_cancel` RPC for reputation-aware rollbacks.
- Cross-chain exchange adapters, light-client crate, indexer with explorer, and benchmark/simulation tools.
- Free-read architecture with receipt batching, execution receipts, governance-tuned CT subsidy ledger accounting, token-bucket rate limiting, and traffic analytics via `gateway.reads_since`.
- Fee-priority mempool with EIP-1559 base fee evolution; high-fee transactions evict low-fee ones and each block nudges the base fee toward a target fullness.
- Bridge primitives with relayer proofs and lock/unlock flows exposed via `blockctl bridge deposit`/`withdraw`.
- Persistent contracts and on-disk key/value state with opcode ABI generation and `contract` CLI for deploy/call.
- DexStore-backed order books and trade logs with multi-hop trust-line routing that scores paths by cost and surfaces fallback routes.
- Governance-tunable mempool fee floor parameters stream to telemetry, explorer history, and rollback logs, while wallet fee warnings emit localized prompts and DID anchors propagate through RPC, CLI, and explorer views.
- CT balance and rate-limit webhooks; mobile light client registers push endpoints and triggers notifications on changes.
- Jittered RPC client with exponential backoff and env-configured timeout windows to prevent request stampedes.
- CI settlement audit job verifying explorer receipt indexes against ledger anchors.
- Fuzz coverage harness that installs LLVM tools on demand and reports missing `.profraw` artifacts.
- Operator runbook for manual DHT recovery detailing peer DB purge, bootstrap reseeding, and convergence checks.

### Immediate

All previously listed directives have been implemented:

- Gossip chaos tests now converge deterministically under 15 % packet loss and
  200 ms jitter with documented tie-break rules (`docs/gossip_chaos.md`) and
  fork-injection fixtures in `tests/net_gossip.rs`.
- Settlement audits index receipts, run periodic verification jobs via
  `tools/settlement_audit`, raise `settle_audit_mismatch_total` alerts, and
  include rollback coverage.
- DHT bootstrapping persists peer databases (`net/discovery.rs`), randomizes
  bootstrap peers, fuzzes identifier exchange, and exposes handshake failure
  metrics.
- Fuzz and chaos tests store reproducible seeds, randomize RPC timeouts, and
  simulate disk-full conditions across storage paths using
  `node/tests/gateway_rate_limit.rs` and `node/tests/storage_repair.rs`.

### Near term

- Launch industrial lane SLA enforcement and dashboard surfacing
  - Enforce deadline slashing for tardy providers via `compute_market::penalize_sla` and persist bonds under `state/market/`.
  - Visualize payout caps and missed jobs in the Grafana network dashboard (`monitoring/grafana/network_dashboard.json`).
  - Track ETAs and on-time percentages per provider with `industrial_rejected_total{reason="SLA"}` and `industrial_eta_seconds` gauges.
  - Ship alerting hooks for SLA violations through Prometheus rules and optional webhooks.
  - Document remediation steps for operators in `docs/operators/incident_playbook.md`.
- Range-boost mesh trials and mobile energy heuristics
  - Prototype BLE/Wi-Fi Direct hop relays in `examples/localnet/` and measure hop counts.
  - Tune lighthouse multipliers based on measured energy usage captured via `mobile_light_client` traces.
  - Log mobile battery and CPU metrics during trials and export `mobile_energy_mwh_total` metrics.
  - Compare mesh performance against baseline deployments, tracking throughput and failure rates.
  - Publish heuristics guidance for application developers in `docs/mobile_light_client.md`.
- Economic simulator runs for emission/fee policy tuning
  - Parameterize inflation and demand scenarios under `sim/src/config/*.toml`.
  - Run Monte Carlo batches via the bench-harness and persist results to `sim/out/`.
  - Report top results to the governance dashboard and archive CSV outputs.
  - Adjust fee curves based on simulation findings with proposals touching `governance/params.rs`.
  - Version-control scenarios for reproducibility under `sim/scenarios/`.
- Compute-backed money and instant-app groundwork
  - Define redeem curves for compute-backed money (CBM) in `docs/economics.md`.
  - Prototype local instant-app execution hooks under `examples/instant_app/`.
  - Record resource consumption metrics for CBM redemption (`cbm_redeem_cpu_seconds`, `cbm_redeem_bytes`).
  - Test edge cases in token-to-CBM conversion via `tests/compute_cbt.rs`.
  - Expose CLI plumbing for CBM redemptions through `blockctl cbm redeem` commands.

### Medium term

- Full cross-chain exchange routing across major assets
  - Implement adapters for SushiSwap and Balancer.
  - Integrate bridge fee estimators and route selectors.
  - Simulate slippage across multi-hop swaps.
  - Provide watchdogs for stuck cross-chain swaps.
  - Document settlement guarantees and failure modes.
- Distributed benchmark network at scale
  - Deploy the harness across 100+ nodes and regions.
  - Automate workload mix permutations.
  - Gather latency and throughput heatmaps.
  - Generate regression dashboards from collected metrics.
  - Publish performance tuning guides.
- Wallet ecosystem expansion
  - Add multisig modules.
  - Ship Swift and Kotlin SDKs for mobile clients.
  - Enable hardware wallet firmware update flows.
  - Provide secure backup and restore tooling.
  - Host an interoperability test suite.
- Governance feature extensions
  - Roll out a staged upgrade pipeline for node versions.
  - Support proposal dependencies and queue management.
  - Add on-chain treasury accounting primitives.
  - Offer community alert subscriptions.
  - Finalize rollback simulation playbooks.
- Mobile light client productionization
  - Optimize header sync and storage footprints.
  - Add push-notification hooks for balance events.
  - Integrate background energy-saving tasks.
  - Support signing and submitting transactions from mobile.
  - Run a beta program across varied hardware.

### Long term

- Smart-contract VM and SDK release
  - Design a deterministic instruction set.
  - Provide gas accounting and metering infrastructure.
  - Release developer tooling and ABI specs.
  - Host example applications and documentation.
  - Perform audits and formal verification.
- Permissionless compute marketplace
  - Integrate heterogeneous GPU/CPU scheduling.
  - Enable reputation scoring for providers.
  - Support escrowed cross-chain payments.
  - Build an SLA arbitration framework.
  - Release marketplace explorer analytics.
- Global jurisdiction compliance framework
  - Publish additional regional policy packs.
  - Support PQ encryption across networks.
  - Maintain transparency logs for requests.
  - Allow per-region feature toggles.
  - Run forkability trials across packs.
- Decentralized storage and bandwidth markets
  - Implement incentive-backed DHT storage.
  - Reward long-range mesh relays.
  - Integrate content addressing for data.
  - Benchmark throughput for large file transfers.
  - Provide client SDKs for retrieval.
- Mainnet launch and sustainability
  - Lock protocol parameters via governance.
  - Run multi-phase audits and bug bounties.
  - Schedule staged token releases.
  - Set up long-term funding mechanisms.
  - Establish community maintenance committees.

## 14. Differentiators
- Utility first: instant wins (works with no bars, instant starts, offline pay, find‑anything) with no partner permission.
- Earn‑by‑helping: proximity and motion become infrastructure; coverage and delivery pay where scarce; compute pays for accepted results.
- Honest money: CBM redeemability; predictable emissions; no backdoors.
- Civic spine: service‑based franchise; catalogs—not protocol—carry social policy; founder exit is verifiable.

## 15 · Outstanding Blockers & Directives

The following items block mainnet readiness and should be prioritized. Each task references canonical file paths for ease of navigation:

1. **Unblock governance CLI builds**
   - Remove the stale `deps` formatter in `node/src/bin/gov.rs`, surface the remaining proposal fields (start/end, vote totals,
     execution flag), and add a regression that builds the CLI under `--features cli`.
2. **Unify wallet Ed25519 dependencies and escrow proof arguments**
   - Align `crates/wallet` on `ed25519-dalek 2.2`, update `wallet::remote_signer` to return the newer `Signature` type, and pass
     `proof.algo` to `verify_proof` in `node/src/bin/wallet.rs`. Add a smoke test under `tests/remote_signer_multisig.rs` once the
     binary links.
3. **Restore light-sync and mempool QoS integration coverage**
   - After fixing the binaries, re-enable `cargo test -p the_block --test light_sync -- --nocapture` and
     `cargo test -p the_block --test mempool_qos -- --nocapture` in CI so regressions in the fee floor and light-client paths are
     caught quickly.
4. **Document targeted CLI build flags in runbooks**
   - Update `docs/testing.md` and `docs/operators/run_a_node.md` with the current feature-gating matrix (`cli`, `telemetry`,
     `gateway`) so operators know how to reproduce the lean build used in integration tests.
5. **Finish telemetry/privacy warning cleanup**
   - Audit modules touched by the recent gating pass (`node/src/service_badge.rs`, `node/src/le_portal.rs`, `node/src/rpc/mod.rs`)
     for lingering `_unused` placeholders and replace them with feature-gated logic or instrumentation so the code stays readable.
6. **Track RPC retry saturation and fault clamps in docs**
   - Keep `docs/networking.md`, `docs/rpc.md`, and `docs/testing.md` aligned with the new `MAX_BACKOFF_EXPONENT` behaviour and
     `[0,1]` fault-rate clamping so operators do not rely on outdated tuning advice.
7. **Verify SimpleDb snapshot safeguards under both features**
   - Add coverage that exercises the atomic rename path with and without `storage-rocksdb` to ensure the recent crash-safe writes
     behave identically across backends.
8. **Stage a docs pass after each regression fix**
   - The build currently fails fast because documentation lags behind implementation; require a `docs/` update in every follow-up
     PR that touches staking, governance, RPC, or telemetry so contributors keep operator guidance accurate.

---
This document supersedes earlier “vision” notes. Outdated references to merchant‑first discounts at TGE, dual‑pool day‑one listings, or protocol‑level backdoors have been removed. The design here aligns all launch materials, SDK plans, marketplace sequencing, governance, legal posture, and networking with the current strategy.

---

## 17 · Agent Playbooks — Consolidated

This section consolidates actionable playbooks from §§18–19. It is included here for single‑file completeness and should be treated as canonical going forward.

### 17.1 Updated Vision & Next Steps

- Phase A (0–2 weeks): Consumer‑first TGE and preview
  - Single USDC/BLOCKc pool seeded and time‑locked; 48h slow‑start; publish pool math/addresses.
  - Dashboard readiness index and bottleneck tile; earnings sparkline; vesting view (if enabled).
  - Shadow marketplace with stake‑backed intents; p25–p75 bands and p_adj; break‑even/margin probe.
  - LocalNet short relays with receipts and paid relays; strict defaults and battery/data caps.
  - Offline money/messaging (canary): escrowed receipts, delayed settlement on reconnect; small group “split later”; SOS broadcast.
- Phase B (2–6 weeks): People‑Built Internet primitives
  - Range Boost delay‑tolerant store‑and‑forward; optional lighthouse recognition; coverage/delivery earnings.
  - Hotspot Exchange: host/guest modes, wrapped traffic; subsidy meters backed by CT.
  - Carry‑to‑Earn sealed bundle courier for commuter routes; privacy explainer; Neighborhood Update Accelerator for instant large downloads.
- Phase C (6–10 weeks): Industrial canary lanes + SDKs v1
  - Transcode and authenticity‑check lanes; sealed‑bid batches; small deposits; per‑slice pricing; daily payout caps; operator diagnostics.
  - SDKs: Provenance; sample apps and docs.
  - Legal/Policy: law‑enforcement guidelines (metadata‑only), transparency log schema, jurisdiction modules, SBOM/licensing, CLA; reproducible builds; privileged RPCs disabled by default; founder irrevocability plan.
- Phase D (10–16 weeks): CBM & Instant Apps; marketplace expansion
  - Daily CBM redeem curves; minimal backstop from marketplace fees.
  - Instant Apps executing via LocalNet; creators paid per CBM use; users often pay zero when contributing.
  - Expand lanes (vector search, diffusion) with caps; auto‑tune p_adj under backlog; batch clearing cadence.

Deliverables Checklist (must‑have artifacts)
- Code: client toggles (LocalNet/Range), escrow/relay/courier receipts, marketplace preview, canary lanes, SDKs.
- Tests: per‑slice pricing, batch clearing, break‑even probes, receipts integrity, range relay, offline settlement, SDK round‑trips.
- Metrics: readiness score, bands, p_adj, coverage/delivery counters, CBM redeem stats, SOS/DM delivery receipts.
- Docs: README/AGENTS/Agents‑Sup alignment; legal/policy folder; governance scaffolding; emissions/CBM docs; SDK guides.

Note: Older “dual pools at TGE,” “merchant‑first discounts,” or protocol‑level backdoor references are obsolete and removed by the Vision above.

### 17.3 Operating Mindset

- Production standard: spec citations, `cargo test --all --features test-telemetry --release`, zero warnings.
- Atomicity and determinism: no partial writes, no nondeterminism.
- Spec‑first: patch specs before code when unclear.
- Logging and observability: instrument changes; silent failures are bugs.
- Security assumptions: treat inputs as adversarial; validations must be total and explicit.
- Granular commits: single logical changes; every commit builds, tests, and lints cleanly.
- Formal proofs: `make -C formal` runs `scripts/install_fstar.sh` (default `v2025.08.07`) which verifies checksums and caches an OS/arch-specific release under `formal/.fstar/<version>`. The installer exports `FSTAR_HOME` so downstream tools can reuse the path; override the pinned release with `FSTAR_VERSION` or set `FSTAR_HOME` to an existing install.
- Monitoring dashboards: run `npm ci --prefix monitoring` then `make -C monitoring lint` (via `npx jsonnet-lint`); CI lints when `monitoring/**` changes and uploads logs as artifacts.
- WAL fuzzing (nightly toolchain required): `make fuzz-wal` stores artifacts and RNG seeds under `fuzz/wal/`; reproduce with `cargo fuzz run wal_fuzz -- -seed=<seed> fuzz/wal/<file>`.
  Use `scripts/extract_wal_seeds.sh` to list seeds and see [docs/wal.md](docs/wal.md) for failure triage.

- Compute market changes: run `cargo nextest run --features telemetry compute_market::courier_retry_updates_metrics price_board` to cover courier retries and price board persistence. Install `cargo-nextest` (compatible with Rust 1.86) if the command is unavailable.
- QUIC networking changes: run `cargo nextest run --profile quic` to exercise QUIC handshake, fanout, and fallback paths. The
  profile enables the `quic` feature flag.

### 17.5 Architecture & Telemetry Highlights (from Agents‑Sup)

- Consensus & Mining: PoW with BLAKE3; dynamic retarget over ~120 blocks with clamp [¼, ×4]; headers carry difficulty; coinbase fields must match tx[0]; decay rewards.
- Accounts & Transactions: Account balances, nonces, pending totals; Ed25519, domain‑tagged signing; `pct_ct` carries an arbitrary 0–100 split with sequential nonce validation.
- Storage: in‑memory `SimpleDb` prototype; schema versioning and migrations; isolated temp dirs for tests.
- Networking & Gossip: QUIC/TCP transport with `PeerSet`; per-peer drop reasons and reputation-aware rate limits surface via `net.peer_stats` and the `net` CLI. JSON‑RPC server in `src/bin/node.rs`; integration tests cover `mempool.stats`, `localnet.submit_receipt`, `dns.publish_record`, `gateway.policy`, and `microshard.roots.last`.
- Inflation subsidies: CT minted per byte, read, and compute with governance-controlled multipliers; reads and writes are rewarded without per-user fees. `industrial_backlog` and `industrial_utilization` metrics, along with `Block::industrial_subsidies()`, surface queued work and realised throughput feeding those multipliers. Ledger snapshots now flow through the CT subsidy store documented in [docs/system_changes.md](docs/system_changes.md#ct-subsidy-unification-2024) and supersede the old `read_reward_pool`. Subsidy multipliers (`beta/gamma/kappa/lambda`) retune each epoch via the formula in `docs/economics.md`; changes are logged under `governance/history` and surfaced in telemetry. An emergency parameter
  `kill_switch_subsidy_reduction` can temporarily scale all multipliers down by
  a voted percentage, granting governance a rapid-response lever during economic
  shocks.
  Operators can inspect current multipliers via the `inflation.params` RPC and
  reconcile stake-weighted payouts by querying `stake.role` for each bonded
  account.
- Telemetry & Spans: metrics including `peer_request_total{peer_id}`,
  `peer_bytes_sent_total{peer_id}`, `peer_drop_total{peer_id,reason}`,
  `peer_handshake_fail_total{peer_id,reason}`,
  `peer_stats_query_total{peer_id}`, `peer_stats_reset_total{peer_id}`,
  `peer_stats_export_total{result}`, `peer_reputation_score{peer_id}`, and
  the `peer_metrics_active` gauge; scheduler metrics `scheduler_match_total{result}`
  and `scheduler_effective_price`; transport metrics `ttl_drop_total`,
  `startup_ttl_drop_total`, `orphan_sweep_total`, `tx_rejected_total{reason=*}`,
  `difficulty_retarget_total`, `difficulty_clamp_total`,
  `quic_conn_latency_seconds`, `quic_bytes_sent_total`,
  `quic_bytes_recv_total`, `quic_handshake_fail_total{peer}`,
  `quic_retransmit_total{peer}`, `quic_cert_rotation_total`,
  `quic_disconnect_total{code}`, `quic_endpoint_reuse_total`;
  release metrics `release_quorum_fail_total` and
  `release_installs_total`; aggregator misses
  `log_correlation_fail_total` feed ops alerts; spans for mempool
  and rebuild flows; Prometheus exporter via `serve_metrics`. Snapshot operations
  export `snapshot_duration_seconds`, `snapshot_fail_total`, and the
  `snapshot_interval`/`snapshot_interval_changed` gauges.
- Schema Migrations: bump `schema_version` with lossless routines; preserve fee invariants; update docs under `docs/schema_migrations/`.
- Python Demo: `PurgeLoop` context manager with env controls; demo integration test settings and troubleshooting tips.
- Quick start: `just demo` runs the Python walkthrough after `./scripts/bootstrap.sh` and fails fast if the virtualenv is missing.
- Governance CLI: `gov submit`, `vote`, `exec`, and `status` persist proposals under `examples/governance/proposals.db`.
- Workload samples under `examples/workloads/` demonstrate slice formats and can
  be executed with `cargo run --example run_workload <file>`; rerun these examples after modifying workload code.

## 18 · Strategic Pillars

- **Governance & Subsidy Economy** ([docs/governance.md](docs/governance.md))
  - [x] Inflation governors tune β/γ/κ/λ multipliers
  - [x] Multi-signature release approvals with persisted signer sets, explorer history, and CLI tooling
  - [ ] On-chain treasury and proposal dependencies
  - Progress: 92%
  - ⚠️ Focus: wire treasury disbursements and dependency visualisations into explorer timelines while finalising external submission workflows.
- **Consensus & Core Execution** ([node/src/consensus](node/src/consensus))
  - [x] UNL-based PoS finality gadget
  - [x] Validator staking & governance controls
  - [x] Integration tests for fault/rollback
  - [x] Release rollback helper ensures binaries revert when provenance validation fails
  - Progress: 89%
  - **Networking & Gossip** ([docs/networking.md](docs/networking.md))
    - [x] QUIC transport with TCP fallback
    - [x] Mutual TLS certificate rotation, diagnostics RPC/CLI, and chaos testing harness
    - [x] Per-peer rate-limit telemetry, cluster `metrics-aggregator`, and CLI/RPC introspection
    - [ ] Large-scale WAN chaos testing
    - Progress: 95%
- **Storage & Free-Read Hosting** ([docs/storage.md](docs/storage.md))
  - [x] Read acknowledgements, WAL-backed stores, and crash-safe snapshot rewrites that stage via fsync’d temp files before promoting base64 images
  - [ ] Incentive-backed DHT marketplace
  - Progress: 83%
  - **Compute Marketplace & CBM** ([docs/compute_market.md](docs/compute_market.md))
    - [x] Capability-aware scheduler with reputation weighting and graceful job cancellation
    - [x] Fee floor enforcement with per-sender slot limits, percentile-configurable windows, wallet telemetry, and eviction audit trails
    - [ ] SLA arbitration and heterogeneous payments
    - Progress: 88%
- **Smart-Contract VM** ([node/src/vm](node/src/vm))
  - [x] Runtime scaffold & gas accounting
  - [x] Contract deployment/execution
  - [x] Tooling & ABI utils
  - Progress: 82%
- **Trust Lines & DEX** ([docs/dex.md](docs/dex.md))
  - [x] Authorization-aware trust lines and order books
  - [ ] Cross-chain settlement proofs
  - Progress: 81%
- **Cross-Chain Bridges** ([docs/bridges.md](docs/bridges.md))
  - [x] Lock/unlock mechanism
  - [x] Light client verification
  - [ ] Relayer incentives
  - Progress: 52%
  - **Wallets** ([docs/wallets.md](docs/wallets.md))
    - [x] CLI enhancements
    - [x] Hardware wallet integration
    - [x] Remote signer workflows
    - Progress: 94%
    - ⚠️ Focus: round out multisig UX (batched signer discovery, richer operator messaging) before tagging the next CLI release.
  - **Monitoring, Debugging & Profiling** ([docs/monitoring.md](docs/monitoring.md))
    - [x] Prometheus/Grafana dashboards and cluster metrics aggregation
    - [x] Metrics-to-logs correlation with automated log dumps on QUIC anomalies
    - [ ] Automated anomaly detection
    - Progress: 88%
  - **Performance** ([docs/performance.md](docs/performance.md))
    - [x] Consensus benchmarks
    - [ ] VM throughput measurements
    - [x] Profiling harness
    - [x] QUIC loss benchmark comparing TCP vs QUIC under chaos
    - Progress: 77%

### Troubleshooting: Missing Tests & Dependencies

- If `cargo test --test <name>` reports *no test target*, the file likely sits at the
  workspace root. Move the test under the crate that owns the code (e.g.
  `node/tests/<name>.rs`) and invoke `cargo test -p node --test <name>`.
- `libp2p_core` and `jsonrpc_core` imports must resolve to crates declared in
  `node/Cargo.toml`. Prefer `libp2p::PeerId` over `libp2p_core::PeerId` and add
  `jsonrpc-core` when RPC modules depend on it.
- Metrics modules are behind the optional `telemetry` feature. Guard any
  `crate::telemetry::*` imports and counters with `#[cfg(feature = "telemetry")]`
  so builds without telemetry succeed.
