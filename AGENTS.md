# AGENTS.md — **The‑Block** Developer Handbook

Quick Index
- Vision & Strategy: see §16
- Agent Playbooks: see §17
- Strategic Pillars: see §18
- Monitoring Stack: see `docs/monitoring.md` and `make monitor`

> **Read this once, then work as if you wrote it.**  Every expectation, switch, flag, and edge‑case is documented here.  If something is unclear, the failure is in this file—open an issue and patch the spec *before* you patch the code.

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

- Rust 1.82+, `cargo-nextest`, `cargo-fuzz` (nightly), and `maturin` for Python bindings.
- Python 3.12.3 in a virtualenv; bootstrap creates `bin/python` shim and prepends `.venv/bin` to `PATH`.
- Node 18+ for the monitoring stack; `npm ci --prefix monitoring` must succeed when `monitoring/**` changes.
- On Linux, `patchelf` is required for wheel installs (bootstrap installs it automatically).
### Disclaimer → Production Readiness Statement

No longer a toy. The‑Block codebase targets production-grade deployment under real economic value.
Every commit is treated as if main-net launch were tomorrow: formal proofs, multi-arch CI, and external security audits are mandatory gates.
Proceed only if you understand that errors here translate directly into on-chain financial risk.

### Vision Snapshot

*The-Block* ultimately targets a civic-grade chain: a one-second base layer
that anchors notarized micro-shards, dual Consumer/Industrial tokens, and a
service-credit meter that rewards honest node work. Governance follows the
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
- Readiness trips Industrial (nodes, capacity, liquidity, vote sustained N days): arm 72h countdown; list USDC/BLOCKi; auto‑convert escrows; start canary lanes; coupons act as rebates.
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

Mainnet readiness: ~94/100 · Vision completion: ~68/100.

**Recent**

- Stake-weighted PoS finality with validator registration, bonding/unbonding, and slashing RPCs.
- Proof-of-History tick generator and Turbine-style gossip for deterministic propagation.
- Parallel execution engine with optional GPU hash workloads.
- Modular wallet framework with hardware signer support and CLI utilities.
- Cross-chain exchange adapters, light-client crate, indexer with explorer, and benchmark/simulation tools.

### Immediate

All previously listed directives have been implemented:

- Gossip chaos tests now converge deterministically under 15 % packet loss and
  200 ms jitter with documented tie-break rules and fork-injection fixtures.
- Credit issuance is governed by validator votes and rewards from read receipts
  removed and migration tooling provided.
- Settlement audits index receipts, run periodic verification jobs, raise
  `settle_audit_mismatch_total` alerts, and include rollback coverage.
- DHT bootstrapping persists peer databases, randomizes bootstrap peers, fuzzes
  identifier exchange, and exposes handshake failure metrics.
- Fuzz and chaos tests store reproducible seeds, randomize RPC timeouts, and
  simulate disk-full conditions across storage paths.

### Near term

- Launch industrial lane SLA enforcement and dashboard surfacing
  - Enforce deadline slashing for tardy providers.
  - Visualize payout caps and missed jobs in the dashboard.
  - Track ETAs and on-time percentages per provider.
  - Ship alerting hooks for SLA violations.
  - Document remediation steps for operators.
- Range-boost mesh trials and mobile energy heuristics
  - Prototype BLE/Wi-Fi Direct hop relays.
  - Tune lighthouse multipliers based on measured energy usage.
  - Log mobile battery and CPU metrics during trials.
  - Compare mesh performance against baseline deployments.
  - Publish heuristics guidance for application developers.
- Economic simulator runs for emission/fee policy tuning
  - Parameterize inflation and demand scenarios.
  - Run Monte Carlo batches via the bench-harness.
  - Report top results to the governance dashboard.
  - Adjust fee curves based on simulation findings.
  - Version-control scenarios for reproducibility.
- Compute-backed money and instant-app groundwork
  - Define redeem curves for compute-backed money (CBM).
  - Prototype local instant-app execution hooks.
  - Record resource consumption metrics for CBM redemption.
  - Test edge cases in credit-to-CBM conversion.
  - Expose CLI plumbing for CBM redemptions.
- Public testnet with PoH/Turbine and parallel executor
  - Package node configurations for external testers.
  - Track time-to-finality and fork rates.
  - Collect validator feedback on GPU workloads.
  - Iterate on network parameters from telemetry.
  - Host weekly syncs summarizing testnet findings.

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
  - Add remote signer and multisig modules.
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
  - Add push-notification hooks for credit events.
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


- Implement free-read accounting: replace gateway budget deductions with
  `ReadReceipt`s, mint credits from a `read_reward_pool`, and add rate-limit
  safeguards before enabling production traffic.

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
  - Hotspot Exchange: host/guest modes, wrapped traffic; credit meters backed by BLOCKc.
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

- Compute market changes: run `cargo nextest run --features telemetry compute_market::courier_retry_updates_metrics price_board` to cover courier retries and price board persistence. Install `cargo-nextest` (v0.9.97-b.2 works with Rust 1.82) if the command is unavailable.

### 17.5 Architecture & Telemetry Highlights (from Agents‑Sup)

- Consensus & Mining: PoW with BLAKE3; dynamic retarget over ~120 blocks with clamp [¼, ×4]; headers carry difficulty; coinbase fields must match tx[0]; decay rewards.
- Accounts & Transactions: Account balances, nonces, pending totals; Ed25519, domain‑tagged signing; `fee_selector` with sequential nonce validation.
- Storage: in‑memory `SimpleDb` prototype; schema versioning and migrations; isolated temp dirs for tests.
- Networking & Gossip: minimal TCP gossip with `PeerSet` and `Message`; JSON‑RPC server in `src/bin/node.rs`; integration tests for gossip and RPC. RPC methods cover `mempool.stats`, `localnet.submit_receipt`, `dns.publish_record`, `gateway.policy`, and `microshard.roots.last`.
- Credits: ledger with governance-controlled issuance, decay, and per-source expiry; reads remain free while providers earn from a read reward pool and writes burn credits.
- Telemetry & Spans: metrics including `ttl_drop_total`, `startup_ttl_drop_total`, `orphan_sweep_total`, `tx_rejected_total{reason=*}`; spans for mempool and rebuild flows; Prometheus exporter via `serve_metrics`. Snapshot operations export `snapshot_duration_seconds`, `snapshot_fail_total`, and the `snapshot_interval`/`snapshot_interval_changed` gauges.
- Schema Migrations: bump `schema_version` with lossless routines; preserve fee invariants; update docs under `docs/schema_migrations/`.
- Python Demo: `PurgeLoop` context manager with env controls; demo integration test settings and troubleshooting tips.
- Quick start: `just demo` runs the Python walkthrough after `./scripts/bootstrap.sh` and fails fast if the virtualenv is missing.
- Governance CLI: `gov submit`, `vote`, `exec`, and `status` persist proposals under `examples/governance/proposals.db`.
- Workload samples under `examples/workloads/` demonstrate slice formats and can
  be executed with `cargo run --example run_workload <file>`; rerun these examples after modifying workload code.

## 18 · Strategic Pillars

- **Consensus Upgrade** ([node/src/consensus](node/src/consensus))
  - [x] UNL-based PoS finality gadget
  - [x] Validator staking & governance controls
  - [ ] Integration tests for fault/rollback
  - Progress: 60%
- **Smart-Contract VM** ([node/src/vm](node/src/vm))
  - [ ] Runtime scaffold & gas accounting
  - [ ] Contract deployment/execution
  - [ ] Tooling & ABI utils
  - Progress: 5%
- **Bridges** ([docs/bridges.md](docs/bridges.md))
  - [ ] Lock/unlock mechanism
  - [ ] Light client verification
  - [ ] Relayer incentives
  - Progress: 5%
- **Wallets** ([docs/wallets.md](docs/wallets.md))
  - [x] CLI enhancements
  - [x] Hardware wallet integration
  - [x] Key management guides
  - Progress: 80%
- **Performance** ([docs/performance.md](docs/performance.md))
  - [x] Consensus benchmarks
  - [ ] VM throughput measurements
  - [x] Profiling harness
  - Progress: 60%
