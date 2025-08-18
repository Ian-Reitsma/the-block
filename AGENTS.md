# AGENTS.md — **The‑Block** Top 0.01 % Developer Handbook

Quick Index (Authoritative Sections)
- Vision & Strategy: see §16
- Agent Playbooks (consolidated): see §17
- Full Playbooks (verbatim): see §§18–19
- Audit & Risks (verbatim): see Appendix at end

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
16. [Vision & Strategy (Authoritative)](#16-vision--strategy-authoritative)
17. [Agent Playbooks — Consolidated](#17-agent-playbooks--consolidated)
18. [Full Agent-Next-Instructions (verbatim)](#18-full-agent-next-instructions-verbatim)
19. [Full Agents-Sup (verbatim)](#19-full-agents-sup-verbatim)
20. [Audit & Risk Notes (verbatim)](#20-audit--risk-notes-verbatim)
21. [API Changelog (verbatim)](#21-api-changelog-verbatim)

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
| Developer Ergonomics ⇢ 0.01 % tier | just dev boots a fully-synced, mining-enabled node with live dashboards < 10 s. | New contributor time-to-first-commit ≤ 15 min. |

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
service-based governance stack. See §16 “Vision & Strategy (Authoritative)”
for the complete blueprint embedded in this document.

### Vision Alignment & Next Steps (Updated)

This repo now converges on a consumer‑first launch, then staged Industrial lanes, plus a people‑built internet that rewards proximity and coverage.

- People‑Built Internet: implement LocalNet (fast road: bonded uplinks, caching, paid relays) and Range Boost (long road: delay‑tolerant store‑and‑forward; optional lighthouse dongles). Coverage earns more per byte where scarce.
- Compute Marketplace: price per slice with sealed‑bid batch matching; bring two canary lanes first (transcode, authenticity checks); expand with caps.
- Compute‑Backed Money: daily redeem curves—X BLOCK buys Y seconds of standard compute or Z MB delivered; Instant Apps execute via nearby compute and settle later.
- Consumer‑First TGE: seed a single USDC pool for BLOCKc (LP time‑locked; slow‑start); arm Industrial only when readiness trips (nodes, capacity, liquidity, vote sustained N days).
- SDKs: ship Provenance, Bonded Contact, Commerce, Ownership Card, and AI Minutes SDKs to make the chain practical on day one.
- Governance & Legal: end‑to‑end encryption; no backdoors; catalogs govern discovery; jurisdiction modules; founder exit with burned protocol levers and reproducible builds.

See “Outstanding Blockers & Directives” for concrete work packages mapped to this vision.

---

## 16 · Vision & Strategy (Authoritative)

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
- Bonded Contact: stake‑backed messages (refundable on accept); escrowed leads; spam‑proof inbox.
- Commerce: “Pay with compute” toggle; receipts; instant refunds; hosted checkout + plugins; USDC settlement.
- Ownership Card: dynamic receipts; warranty/transfer; recall alerts; resale provenance.
- AI Minutes: per‑app minutes (transcribe/summarize/enhance) settled behind the scenes by CBM.

## 11. Security, Legal & Governance Posture
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
- Now: 1‑second engine; RPC; purge loops; tests/benches; docs; dual‑token model; launch tooling.
- Next: Consumer TGE; LocalNet + Range Boost; offline money & carrierless messaging; canary lanes; SDK v1; Industrial readiness; coverage bounties; lighthouse reference.
- Later: heavier shards (diffusion/vector search); WISP/ISP gateways; formal proofs of invariants; PQ crypto; broader jurisdiction packs.

## 14. Differentiators
- Utility first: instant wins (works with no bars, instant starts, offline pay, find‑anything) with no partner permission.
- Earn‑by‑helping: proximity and motion become infrastructure; coverage and delivery pay where scarce; compute pays for accepted results.
- Honest money: CBM redeemability; predictable emissions; no backdoors.
- Civic spine: service‑based franchise; catalogs—not protocol—carry social policy; founder exit is verifiable.

---
This document supersedes earlier “vision” notes. Outdated references to merchant‑first discounts at TGE, dual‑pool day‑one listings, or protocol‑level backdoors have been removed. The design here aligns all launch materials, SDK plans, marketplace sequencing, governance, legal posture, and networking with the current strategy.

---

## 17 · Agent Playbooks — Consolidated

This section consolidates actionable playbooks from §§18–19. It is included here for single‑file completeness and should be treated as canonical going forward.

### 17.1 Updated Vision & Authoritative Next Steps

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
  - SDKs: Provenance, Bonded Contact, Commerce, Ownership Card, AI Minutes; sample apps and docs.
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

Note: Older “dual pools at TGE,” “merchant‑first discounts,” or protocol‑level backdoor references are obsolete and removed by the authoritative Vision above.

### 17.2 Completed Roadmap Items (Recap)

- Genesis integrity and bootstrap fixes; fee refactor with 128‑bit accumulators, checksum fields, error codes via PyO3.
- Temp DB isolation; `(sender, nonce)` replay guard in tests.
- Telemetry expansion: metrics exporter and full rejection/loop counters with spans (`mempool_mutex`, `admission_lock`, `eviction_sweep`, `startup_rebuild`).
- Mempool atomicity: `mempool_mutex → sender_mutex` critical section; orphan sweep threshold and rebuild.
- Timestamp persistence and eviction proof: deterministic startup purge; panic‑inject eviction test and lock‑poison recovery.
- Startup TTL purge: `Blockchain::open` rebuild and purge, with restart tests for `ttl_drop_total` and `startup_ttl_drop_total`.
- Cached tx serialized size for fee‑per‑byte; anchor checker improvements; dynamic difficulty per header; Pythonic `PurgeLoop`; cross‑lang serialization determinism.
- Saturating counters at `u64::MAX`; stable `TxAdmissionError` codes with Python `.code` exposure and tests.
- Gossip and JSON‑RPC node with integration tests; RPC flags `--mempool-purge-interval` and `--metrics-addr`.
- Demo test harness that compiles wheels as needed and captures logs on failure.

### 17.3 Operating Mindset

- 0.01% standard: spec citations, `cargo test --all`, zero warnings.
- Atomicity and determinism: no partial writes, no nondeterminism.
- Spec‑first: patch specs before code when unclear.
- Logging and observability: instrument changes; silent failures are bugs.
- Security assumptions: treat inputs as adversarial; validations must be total and explicit.
- Granular commits: single logical changes; every commit builds, tests, and lints cleanly.

### 17.4 Handoff Checklist <a id="174-handoff-checklist"></a>

- Read AGENTS.md fully; bootstrap via `bootstrap.sh`.
- Pick one priority and implement end‑to‑end with tests.
- Run: `cargo fmt`, `black --check .`, `cargo clippy --all-targets --all-features`, `cargo test --all`, and `python scripts/check_anchors.py --md-anchors`.
- Update docs/specs alongside code; add proofs/invariants as needed.
- Open PR with file and command citations per §9 Commit & PR Protocol.
- Demo/test env: set `TB_PURGE_LOOP_SECS=1`, `PYTHONUNBUFFERED=1`, and leave `TB_DEMO_MANUAL_PURGE` unset for context‑managed purge loop; set it to `1` for manual shutdown testing.

### 17.5 Architecture & Telemetry Highlights (from Agents‑Sup)

- Consensus & Mining: PoW with BLAKE3; dynamic retarget over ~120 blocks with clamp [¼, ×4]; headers carry difficulty; coinbase fields must match tx[0]; decay rewards.
- Accounts & Transactions: Account balances, nonces, pending totals; Ed25519, domain‑tagged signing; `fee_selector` with sequential nonce validation.
- Storage: in‑memory `SimpleDb` prototype; schema versioning and migrations; isolated temp dirs for tests.
- Networking & Gossip: minimal TCP gossip with `PeerSet` and `Message`; JSON‑RPC server in `src/bin/node.rs`; integration tests for gossip and RPC.
- Telemetry & Spans: metrics including `ttl_drop_total`, `startup_ttl_drop_total`, `orphan_sweep_total`, `tx_rejected_total{reason=*}`; spans for mempool and rebuild flows; Prometheus exporter via `serve_metrics`.
- Schema Migrations: bump `schema_version` with lossless routines; preserve fee invariants; update docs under `docs/schema_migrations/`.
- Python Demo: `PurgeLoop` context manager with env controls; demo integration test settings and troubleshooting tips.

-
---

## 18 · Full Agent-Next-Instructions (verbatim)

# Agent-Next-Instructions.md — Developer Playbook

> **Read carefully. Execute precisely.**  This file hands off the current
> development state and expectations to the next agent.  Every directive
> presumes you have absorbed `AGENTS.md` and the repository specs.

---

## Updated Vision & Authoritative Next Steps (Supersedes older sections)

This section reflects the unified vision in `agents_vision.md` and overrides older guidance below. Treat it as the single source of truth for sequencing.

### Phase A (0–2 weeks): Consumer‑First TGE + Preview
- Seed a single USDC/BLOCKc pool (1,000,000 BLOCKc : $500 USDC); time‑lock LP; 48h slow‑start; publish pool addresses/math.
- Dashboard: add Readiness Index (nodes, industrial‑capable nodes, pledged liquidity, stake‑backed demand, vote%), bottleneck tile, 7‑day earnings sparkline, vesting view (if pre‑TGE accruals enabled).
- Shadow marketplace: stake‑backed intents for 2–3 job types; compute/display p25–p75 bands + p_adj; break‑even/margin probe (kWh input × local benchmark).
- LocalNet (fast road): short relays over Wi‑Fi/BLE with receipts and paid relays; strict defaults (Wi‑Fi only unless charging) and battery/data caps.
- Offline money & messaging (canary): P2P escrowed receipts, delayed settlement on reconnect; small group tab with “split later;” SOS broadcast.

### Phase B (2–6 weeks): People‑Built Internet Primitives
- Range Boost (long road): delay‑tolerant store‑and‑forward; optional lighthouse recognition (USB/hat radios later); earnings receipts for coverage/delivery.
- Hotspot Exchange: host mode (rate‑limited guest Wi‑Fi, wrapped traffic); guest mode (one‑tap join, credit spend); credits backed by BLOCKc with simple meter.
- Carry‑to‑Earn: bundle courier with sealed delivery receipts; earnings for commuters/routes; privacy explainer in UI.
- Neighborhood Update Accelerator: content‑addressed seeding and instant downloads for big updates/patches/trailers.

### Phase C (6–10 weeks): Industrial Canary Lanes + SDKs v1
- Two Industrial lanes: live transcode and authenticity checks; sealed‑bid batch matches; tiny deposits; per‑slice pricing; daily per‑node payout caps; operator diagnostics.
- SDKs v1: Provenance (sensor‑edge signing + proof bundles), Bonded Contact (stake‑backed inbox), Commerce (pay‑with‑compute + receipts + instant refunds), Ownership Card (warranty/transfer/recall), AI Minutes (per‑app minutes settled by CBM).
- Legal/Policy: Law‑Enforcement Guidelines, transparency log schema, jurisdiction modules (YAML) + client wiring, SBOM/licensing, CLA; reproducible builds, privileged RPCs disabled by default; founder “irrevocability” plan.

### Phase D (10–16 weeks): CBM & Instant Apps; Marketplace Expansion
- CBM curves: daily redeemability—X BLOCK buys Y seconds of standard compute or Z MB delivered; minimal backstop from marketplace fees.
- Instant Apps: transcode, summarize, send‑huge‑file executing via LocalNet and settling later; creators paid per use in CBM.
- Marketplace expansion: heavier lanes (vector search, diffusion) with caps; auto‑tune p_adj under backlog; batch clearing cadence.

### Deliverables Checklist (must‑have artifacts)
- Code: client toggles (LocalNet/Range), escrow receipts, relay/courier receipts, marketplace preview, canary lanes, SDKs.
- Tests: per‑slice pricing, batch clearing, break‑even probes, receipts integrity, range relay, offline settlement, SDK round‑trips.
- Metrics: readiness score, bands, p_adj, coverage/delivery counters, CBM redeem stats, SOS/DM delivery receipts.
- Docs: README/AGENTS/Agents‑Sup alignment; legal/policy folder; governance scaffolding; emissions/CBM docs; SDK guides.

> Note: older “dual pools at TGE,” “merchant‑first discounts,” or protocol‑level backdoor references are obsolete. Follow this plan.

### Vision in Brief

The-Block is building a one-second Layer 1 that notarizes sub-second
micro-shards. Economics use two tradeable tokens—Consumer and
Industrial—plus non-transferable service credits. Governance assigns one
badge per reliable node and uses shard districts for bicameral votes. The
current kernel already covers dynamic difficulty, dual-token fee routing,
purge loops with telemetry, basic gossip, and a JSON-RPC node. The tasks
below extend this foundation toward persistent storage, secure networking,
and full service-based governance.
See §16 for the full narrative on how these pieces cohere into
a civic-grade public good.

---

## Completed Roadmap Items (Recap)

The following fixes are already in `main`.  Use them as context and do not
re‑implement:

1. **Genesis integrity** verified at compile time; bootstrap script fixed.
2. **Fee refactor** and consensus logic overhaul, including drop_transaction,
   fee routing safeguards, 128‑bit fee accumulators, checksum fields, and
   distinct error codes surfaced via PyO3.
3. **Documentation refresh**: disclaimer relocation, agents supplement (§19), and
   schema guidance.
4. **Temp DB isolation** for tests; `Blockchain::new(path)` creates per-run
   directories and `test_replay_attack_prevention` enforces `(sender, nonce)`
   dedup.
5. **Telemetry expansion**: HTTP metrics exporter, `ttl_drop_total`,
   `startup_ttl_drop_total` (expired mempool entries dropped during startup), `lock_poison_total`, `orphan_sweep_total`,
   `invalid_selector_reject_total`, `balance_overflow_reject_total`,
   `drop_not_found_total`, `tx_rejected_total{reason=*}`, and span coverage
   for `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
   `startup_rebuild` capturing sender, nonce, fee_per_byte, and
  mempool_size ([`src/lib.rs`](src/lib.rs#L1067-L1082),
   [`src/lib.rs`](src/lib.rs#L1536-L1542),
   [`src/lib.rs`](src/lib.rs#L1622-L1657),
   [`src/lib.rs`](src/lib.rs#L879-L889)). Comparator ordering test for
   mempool priority.
   `maybe_spawn_purge_loop` (wrapped by the `PurgeLoop` context manager)
   reads `TB_PURGE_LOOP_SECS`/`--mempool-purge-interval` and calls
   `purge_expired` periodically, advancing TTL and orphan-sweep metrics.
   Setting `TB_DEMO_MANUAL_PURGE=1` while running `demo.py` skips the
   context manager and demonstrates manual `ShutdownFlag` + handle
   control instead.
6. **Mempool atomicity**: global `mempool_mutex → sender_mutex` critical section with
   counter updates, heap ops, and pending balances inside; orphan sweeps rebuild
   the heap when `orphan_counter > mempool_size / 2` and emit `ORPHAN_SWEEP_TOTAL`.
7. **Timestamp persistence & eviction proof**: mempool entries persist
   `timestamp_ticks` for deterministic startup purge; panic-inject eviction test
   proves lock-poison recovery.
8. **B‑5 Startup TTL Purge — COMPLETED**: `Blockchain::open` batches mempool rebuilds,
   invokes [`purge_expired`](src/lib.rs#L1597-L1666) on startup
   ([src/lib.rs](src/lib.rs#L918-L935)), and restart tests ensure both
   `ttl_drop_total` and `startup_ttl_drop_total` advance.
9. Cached each transaction's serialized size in `MempoolEntry` and updated
   `purge_expired` to use the cached fee-per-byte, avoiding reserialization.
   `scripts/check_anchors.py --md-anchors` now validates Rust line and
   Markdown section links in CI.
10. Dynamic difficulty retargeting with per-block `difficulty` field,
    in-block nonce continuity validation, Pythonic `PurgeLoop` context
    manager wrapping `ShutdownFlag` and `PurgeLoopHandle`, and
    cross-language serialization determinism tests. `Blockchain::open`,
    `mine_block`, and `import_chain` now refresh the `difficulty`
    field to the current network target.
11. Telemetry counters `TTL_DROP_TOTAL` and `ORPHAN_SWEEP_TOTAL` saturate at
    `u64::MAX`; tests confirm `ShutdownFlag.trigger()` halts purge threads
    before overflow.
12. Stable transaction-admission error codes: `TxAdmissionError` is
    `#[repr(u16)]`, Python re-exports `ERR_*` constants and `.code` attributes,
    and `log_event` emits the numeric `code` in telemetry JSON. A
    table-driven `tests/test_tx_error_codes.py` enumerates every variant and
    asserts `exc.code == ERR_*`; a doc-hidden `poison_mempool(bc)` helper
    enables lock-poison coverage, while `tests/logging.rs` parses telemetry
    JSON to ensure accepted and duplicate transactions carry the expected
    numeric codes.
13. Anchor checker walks `src`, `tests`, `benches`, and `xtask` with cached
    file reads and parallel scanning; `scripts/test_check_anchors.py` covers
    `tests/` anchors. `run_all_tests.sh` auto-detects optional features via
    `cargo metadata | jq` and warns when `jq` or `cargo fuzz` is absent,
    continuing without them.
14. Direct `spawn_purge_loop(bc, secs, shutdown)` binding for Python enables
    manual interval control and concurrency tests. New tests cover manual
    trigger/join semantics, panic propagation via `panic_next_purge`, and
    env-driven loops sweeping TTL-expired and orphan transactions, asserting
    `ttl_drop_total` and `orphan_sweep_total` each advance and the mempool
    returns to zero.
15. Coinbase/fee recomputation is stress-tested: property-based generator
    randomizes blocks, coinbases, and fees, and schema upgrade tests validate
    emission totals and per-block `fee_checksum` after migration.
16. Minimal TCP gossip layer under `net/` broadcasts transactions and blocks,
    applies a longest-chain rule on conflicts, and has a multi-node
    convergence test.
17. Command-line `node` binary exposes `tokio`-based JSON-RPC endpoints for balance queries,
    transaction submission, mining control, and metrics; `--mempool-purge-interval`
    and `--metrics-addr` flags configure purge loop and Prometheus export.
18. `tests/node_rpc.rs` performs a smoke test of the RPC layer, exercising the
    metrics, balance, and mining-control methods.
19. `demo_runs_clean` integration test injects `TB_PURGE_LOOP_SECS=1`, sets
    `TB_DEMO_MANUAL_PURGE` to the empty string, forces unbuffered Python output,
    enforces a 10-second timeout, and prints demo logs on failure while
    preserving them on disk.
20. `tests/test_spawn_purge_loop.py` spawns two manual purge loops with
    different intervals and cross-order joins to assert mempool invariants and metrics.

---

## Mid‑Term Roadmap — 2‑6 Months
Once the immediate blockers are merged, build outward while maintaining determinism and observability established above.

1. **Durable storage backend** – replace `SimpleDb` with a crash-safe key‑value store. B‑3’s timestamp persistence is prerequisite.
2. **P2P networking & sync** – design gossip and fork-resolution protocols; a race-free mempool and replay-safe persistence prevent divergence.
3. **Node API & tooling** – expose RPC/CLI once telemetry counters and spans enable operational monitoring.
4. **Dynamic difficulty retargeting — COMPLETED** – moving-average algorithm with clamped adjustment now governs PoW targets.
5. **Enhanced validation & security** – extend panic-inject and fuzz coverage to network inputs, enforcing signature, nonce, and fee invariants.
6. **Testing & visualization tools** – multi-node integration tests and dashboards leveraging the metrics emitted above.

## Long‑Term Vision — 6 + Months

These require research but should influence architectural choices now:

1. **Quantum‑resistant cryptography**: modular signature/hash back‑ends and
   dual‑algorithm migration paths.
2. **Proof‑of‑resource extensions**: reward storage/bandwidth/service alongside
   PoW.
3. **Layered or sharded ledger architecture**: child chains, rollups, or
   periodic checkpoints to a main chain.
4. **On‑chain governance & upgradability**: voting mechanisms, fork scheduling,
   and automatic parameter activation.

---

## Project Completion Snapshot — 60 / 100

The kernel is progressing but still far from investor-ready. Score components:

* **Core consensus/state/tx logic** ≈ 94 %: fee routing, nonce continuity, dynamic difficulty, and comparator ordering are proven.
* **Database/persistence** ≈ 65 %: schema v4 migration exists, but durable backend and rollback tooling are absent.
* **Testing/validation** ≈ 70 %: comparator, panic-inject, and serialization equivalence tests exist; long-range fuzz gaps remain.
* **Demo/docs** ≈ 68 %: demo narrates fee selectors and purge loop; cross-links and startup rebuild details still missing.
* **Networking (P2P/sync/forks)** 0 %: no gossip, fork resolution, handshake, or RPC/CLI.
* **Mid-term engineering infra** ≈ 20 %: CI enforces fmt/tests and serialization determinism but lacks coverage, fuzz, or schema lint.
* **Upgrade/governance** 0 %: no fork artifacts, snapshot tools, or governance docs.
* **Long-term vision** 0 %: quantum safety, resource proofs, sharding, and on-chain governance remain conceptual only.

**Milestone map**

* `0‑40` → R&D, spec, core consensus *(complete)*.
* `40‑60` → DB, fee/migration/schema/atomicity *(current, >90 % done)*.
* `60‑80` → P2P, CLI/RPC, persistent DB, testnet harness, governance stubs.
* `80‑100` → Mainnet readiness: burn-in, ops, monitoring, audit, launch.

---

## Investor‑Ready Milestone — 70 / 100

To reach 70 / 100, deliver the following:

1. **Proven core** — invariants, migrations, and audit artifacts checked in.
2. **Persistent storage** — crash-proof DB with snapshot/rollback tooling.
3. **Networked operation** — nodes gossip, sync, and fork-resolve in practice.
4. **CLI/RPC usability** — non-core devs can run and query nodes easily.
5. **CI, fuzz, coverage** — automated tests block regressions and enforce
   cross-language determinism.
6. **Docs & roadmap** — README/AGENTS/specs explain uniqueness, scale, and
   security; gaps are candid.
7. **Upgrade signaling** — feature-bit or handshake mechanism for forks.

---

## Codex-Assisted Networking & Test Harness

Codex can scaffold networking, but **you** define the protocol and invariants.

1. **Specify message schema**
   - Draft `TxBroadcast`, `BlockAnnounce`, `ChainRequest`, `FeatureHandshake`
     structs with `serde` tags and domain-separated signature bytes.
   - Document handshake versioning, fork-choice, and error codes in `spec/`.
2. **Generate networking code**
   - Ask Codex to emit Rust (`tokio`/`libp2p`) or Python `asyncio` sockets that
     serialize/deserialize the structs.
   - Enforce signature and domain checks on every inbound payload.
3. **Script multi-node launch**
   - Codex writes bash/Python to spawn N nodes with configs (port, peerlist,
     node key).  Use local IPs or SSH to remote VMs.
4. **Inject adversarial traffic**
   - Automate replay, double-spend, fork, flood, and peer-drop scenarios.
   - State hashes across nodes must converge after each run.
5. **Aggregate results**
   - Codex collects logs and snapshots, diffing them for divergence.
   - Any nondeterminism or crash is a release blocker.

This loop upgrades the kernel into a true devnet with reproducible tests.

---

## Execution Timeline & Risk Budget

Estimated effort (buffer +20 % for network bugs):

* Persistent storage backend (real DB, snapshot, rollback) — 7‑10 days.
* Mempool concurrency & atomicity hardening — 2‑3 days.
* Transaction admission/validation edge cases & logging — 2‑3 days.
* P2P networking & chain sync (libp2p, gossip, fork logic) — 10‑14 days.
* CLI/RPC & tooling — 3‑5 days.
* Comprehensive tests, fuzzing, invariants, multi-node integration — 7‑10 days.
* Demo & documentation polish — 2‑4 days.
* CI/CD and release hygiene (coverage, lint, schema, build matrix) — 3‑5 days.

Risks: networking bugs, testnet nondeterminism, DB corruption, and doc polish
can extend timelines.

---

## Comparative Positioning

**Scores**: Solana 84, Ethereum 68, Bitcoin 50, Pi Network 25, The‑Block 60.

**Structural advantages**

* Spec rigor with cross-language determinism.
* Dual-token economics unique among majors.
* Quantum-ready hooks for future cryptography.
* Rust-first safety with `#![forbid(unsafe_code)]`.
* Canonical fee checksum per block.

**Current deficits**

* Rudimentary P2P without peer discovery or robust sync.
* In-memory DB; no persistent storage backend.
* Absent upgrade governance and tooling.

**Path to 80 +**: expand networking & sync (+10), persistent storage (+5),
CLI/RPC/explorer enhancements (+3), governance artifacts (+4),
testnet burn-in & audits (+5), ecosystem tooling (+5).

| Chain | Score | Strengths                | Liabilities                   |
|-------|-------|--------------------------|-------------------------------|
| Solana | 84 | high TPS, sub-sec finality | hardware-heavy, outages      |
| Ethereum | 68 | deep ecosystem, rollups   | 15 TPS base, high fees       |
| Bitcoin | 50 | longest uptime, strong PoW | 10‑min blocks, limited script|
| Pi Network | 25 | large funnel, mobile UX   | opaque consensus, closed code|
| The‑Block | 60 | spec-first, dual-token, Rust | basic gossip, mem DB |

---


## Operating Mindset

* **0.01 % standard**: every commit must be justifiable via spec citations and
  must leave the repo buildable (`cargo test --all`).
* **Atomicity & determinism**: no partial state writes, no nondeterministic
  behavior.
* **Spec-first**: if the spec is unclear, patch the spec before writing code.
* **Logging & observability**: instrument changes; silent failures are bugs.
* **Security**: assume adversarial inputs.  All validation paths must be total
  and explicit.
* **Zero TODOs**: resolve or ticket TODO/FIXME before merge; repo must remain
  warning-free.
* **Warnings = errors**: clippy and compiler warnings block merges.
* **Granular commits**: one logical change per commit; every commit passes all
  tests and lints.

---

## Handoff Checklist for the Next Agent

1. Reread `AGENTS.md` in full and set up the environment with `bootstrap.sh`.
2. Confirm no `AGENT_NOTES.md` file exists; if one appears, read it.
3. Select a single immediate priority and implement it end‑to‑end with tests.
4. Run `cargo fmt`, `black --check .`, `cargo clippy --all-targets --all-features`,
   `cargo test --all`, and `python scripts/check_anchors.py --md-anchors`
   before committing. If `black --check` flags `scripts/check_anchors.py` or
   `tests/test_tx_error_codes.py`, format them with `black scripts/check_anchors.py
   tests/test_tx_error_codes.py`, commit the result, and rerun `black --check .`
   to confirm a clean tree.
5. Update docs and specs alongside code.  Every new invariant needs a proof or
   reference in §19 or the appropriate spec.
6. Open a PR referencing this file in the summary, detailing tests and docs.
7. Include file and command citations in the PR per `AGENTS.md` §9.
8. When running `demo.py` (e.g., the `demo_runs_clean` test), set
    `TB_PURGE_LOOP_SECS` to a positive integer such as `1` so the purge
    loop context manager can spawn, force `PYTHONUNBUFFERED=1` for
    real-time logs, and leave `TB_DEMO_MANUAL_PURGE` unset or empty to
    use the context manager; set `TB_DEMO_MANUAL_PURGE=1` to exercise the
    manual shutdown‑flag/handle example instead. The script will invoke
    `maturin develop` automatically if the `the_block` module is missing.
    If `cargo test` hangs on `demo_runs_clean`, run
    `cargo test demo_runs_clean -- --nocapture` once to build the Python
    wheel before rerunning the full suite.
9. For long-running tests (e.g., `reopen_from_snapshot`), set `TB_SNAPSHOT_INTERVAL`
    and lower block counts locally to iterate quickly, then restore canonical
    values before committing.

Stay relentless.  Mediocrity is a bug.

---

## 19 · Full Agents-Sup (verbatim)

# Agents Supplement — Strategic Roadmap and Orientation

This document extends `AGENTS.md` with a deep dive into the project's long‑term vision and the immediate development sequence. Read both files in full before contributing.

> Update: This supplement aligns with the unified vision in `agents_vision.md`. The “Vision Alignment & Next Steps” section below is authoritative and supersedes older roadmap fragments.

## Vision Alignment & Next Steps (Authoritative)

### People‑Built Internet
- LocalNet (fast road): bonded uplinks, caching, paid relays; strict mobile defaults; receipts and rate‑limits; metrics in Dashboard.
- Range Boost (long road): delay‑tolerant store‑and‑forward; optional lighthouse radios; coverage/delivery earnings; coverage heatmap.
- Carry‑to‑Earn: bundle courier with sealed delivery receipts; commuter routes; privacy explainer.
- Hotspot Exchange: host/guest modes; wrapped traffic; credit meters backed by BLOCKc.
- Neighborhood Update Accelerator: content‑addressed seeding for instant updates/patches.

### Compute Marketplace & CBM
- Shadow intents (stake‑backed) show p25–p75 bands + p_adj. At Industrial TGE, convert escrows to BLOCKi and start two canary lanes (transcode, authenticity). Daily per‑node caps and operator diagnostics.
- Compute‑Backed Money (CBM): daily redeem curves (X BLOCK → Y seconds or Z MB); minimal backstop from marketplace fees; Instant Apps execute via LocalNet and settle later.

### Launch & SDKs
- Consumer‑first TGE: single USDC/BLOCKc pool (1,000,000 : $500), LP time‑lock, 48h slow‑start; publish pool math/addresses. Industrial armed when readiness (nodes/capacity/liquidity/vote) sustains N days.
- SDKs v1: Provenance, Bonded Contact, Commerce, Ownership Card, AI Minutes; sample apps + docs.

### Governance & Legal
- Service‑tied badges; bicameral votes; catalog governance (list/delist logs); treasury streaming.
- Law‑Enforcement Guidelines (metadata‑only); transparency log; jurisdiction modules (client/provider); SBOM/licensing; CLA.
- Founder exit milestones: burn protocol admin keys; reproducible builds; disable privileged RPCs; publish irrevocability txs.

### Deliverables
- Code/tests/metrics/docs as listed in Agent‑Next‑Instructions “Updated Vision & Authoritative Next Steps.”

## 0. Scope Reminder

* **Production Kernel** – The code targets real economic deployment. It is **not** a toy network nor financial instrument.
* **Rust First, Python Friendly** – The kernel is implemented in Rust with PyO3 bindings for scripting and tests. Absolutely no unsafe code is allowed.
* **Dual‑Token Ledger** – Balances are tracked in consumer and industrial units. Token arithmetic uses the `TokenAmount` wrapper.

## 1. Current Architecture Overview

### Consensus & Mining
* Proof of Work using BLAKE3 hashes with dynamic difficulty retargeting.
  `expected_difficulty` computes a moving average over ~120 block timestamps
  clamped to a \[¼, ×4] adjustment; headers store the difficulty and validators
  reject mismatches. See [`CONSENSUS.md#difficulty-retargeting`](CONSENSUS.md#difficulty-retargeting)
  for the full algorithm and tuning parameters.
* Each block stores `coinbase_consumer` and `coinbase_industrial`; the first transaction must match these values.
* Block rewards decay by a factor of `DECAY_NUMERATOR / DECAY_DENOMINATOR` each block.

### Accounts & Transactions
* `Account` maintains balances, nonce and pending totals to prevent overspending.
* `RawTxPayload` → `SignedTransaction` using Ed25519 signatures. The canonical signing bytes are `domain_tag || bincode(payload)`.
* Transactions include a `fee_selector` selector (0=consumer, 1=industrial, 2=split) and must use sequential nonces; `validate_block`
  tracks expected nonces per sender and rejects gaps or repeats within a block.

### Storage
* Persistent state lives in an in-memory map (`SimpleDb`). `ChainDisk` encapsulates the
  chain, account map and emission counters. Schema version = 3.
* `Blockchain` tracks its `path` and its `Drop` impl removes the directory.
  `Blockchain::new(path)` expects a unique temp directory; tests use
  `tests::util::temp::temp_dir()` to avoid cross-test leakage and ensure
  automatic cleanup.

### Mempool Concurrency
* A global `mempool_mutex` guards all mempool mutations before the per-sender
  lock. Counter updates, heap pushes/pops, and pending balance/nonces are
  executed inside this lock order, ensuring the invariant `mempool_size ≤
  max_mempool_size`.
* Entries referencing missing accounts increment an `orphan_counter`; once the
  counter exceeds half the mempool, a sweep drops all orphans, emits
  `ORPHAN_SWEEP_TOTAL`, and resets the counter.
* Each mempool entry caches its serialized size so `purge_expired` can compute
  fee-per-byte without reserializing transactions.

### Networking & Gossip
* The `net` module provides a minimal TCP gossip layer with a thread-safe
  `PeerSet` and `Message` enums for `Hello`, `Tx`, `Block`, and `Chain`.
* Nodes broadcast transactions and blocks and adopt longer forks via
  `Blockchain::import_chain`, ensuring convergence on the longest chain.
* `src/bin/node.rs` wraps the chain in a `tokio`-based JSON-RPC server exposing balance queries,
  transaction submission, start/stop mining, and metrics export. Flags
  `--mempool-purge-interval` and `--metrics-addr` configure the purge loop and
  Prometheus endpoint.
* Integration test `tests/net_gossip.rs` spawns three nodes that exchange
  data and verify equal chain heights.
* `tests/node_rpc.rs` smoke-tests the RPC layer by hitting the metrics,
  balance, and mining-control endpoints.

### Telemetry Metrics & Spans
* Metrics: `mempool_size`, `evictions_total`, `fee_floor_reject_total`,
  `dup_tx_reject_total`, `ttl_drop_total`, `startup_ttl_drop_total`
  (expired mempool entries dropped during startup),
  `lock_poison_total`, `orphan_sweep_total`, `invalid_selector_reject_total`,
  `balance_overflow_reject_total`, `drop_not_found_total`,
  `tx_rejected_total{reason=*}`. `ttl_drop_total` and `orphan_sweep_total`
  saturate at `u64::MAX` to avoid overflow.
* `maybe_spawn_purge_loop` reads `TB_PURGE_LOOP_SECS` (or
  `--mempool-purge-interval`) and spawns a thread that periodically calls
  `purge_expired`, advancing TTL and orphan-sweep metrics even when the node is
  idle. Python exposes a `PurgeLoop` context manager wrapping
  `ShutdownFlag`/`PurgeLoopHandle` for automatic startup and clean shutdown;
  manual control is available via the `spawn_purge_loop(bc, secs, shutdown)`
  binding. Set `TB_DEMO_MANUAL_PURGE=1` while running `demo.py` to opt
  into the manual flag/handle demonstration.
* Admission failures are reported with `TxAdmissionError` which is
  `#[repr(u16)]`; Python re-exports `ERR_*` constants and each exception has a
  `.code` attribute. `log_event` includes the same numeric `code` in telemetry
  JSON so log consumers can match on stable identifiers. The
  `tests/test_tx_error_codes.py` suite iterates over every variant to assert
  `exc.code == ERR_*`, a doc-hidden `poison_mempool(bc)` helper enables
  lock-poison coverage, and `tests/logging.rs` captures telemetry JSON for
  admitted transactions, duplicates, nonce gaps, insufficient balances, and
  purge-loop TTL and orphan sweeps.
* Sample JSON logs (`--features telemetry-json`):

  ```json
  {"op":"reject","sender":"a","nonce":3,"reason":"nonce_gap","code":3}
  {"op":"purge_loop","reason":"ttl_drop_total","code":0,"fpb":1}
  {"op":"purge_loop","reason":"orphan_sweep_total","code":0,"fpb":1}
  ```
* Spans: `mempool_mutex` (sender, nonce, fpb, mempool_size),
  `admission_lock` (sender, nonce), `eviction_sweep` (sender, nonce,
  fpb, mempool_size), `startup_rebuild` (sender, nonce, fpb,
  mempool_size). See [`src/lib.rs`](src/lib.rs#L1067-L1082),
  [`src/lib.rs`](src/lib.rs#L1536-L1542),
  [`src/lib.rs`](src/lib.rs#L1622-L1657), and
  [`src/lib.rs`](src/lib.rs#L879-L889).
* `serve_metrics(addr)` exposes Prometheus text; e.g.
  `curl -s localhost:9000/metrics | grep tx_rejected_total`.
  The CLI uses `--metrics-addr` to spawn this exporter during `node run`.

### Schema Migrations & Invariants
* Bump `ChainDisk.schema_version` for any on-disk format change and supply a lossless migration routine with tests.
* Each migration must preserve [`INV-FEE-01`](ECONOMICS.md#inv-fee-01) and [`INV-FEE-02`](ECONOMICS.md#inv-fee-02); update `docs/schema_migrations/` with the new invariants.

### Python Demo
* `demo.py` creates a fresh chain, mines a genesis block, signs a sample
  message, submits a transaction and mines additional blocks while
  printing explanatory output. It uses `with PurgeLoop(bc):` to spawn and
  join the purge thread automatically. Metric assertions require building
  the module with `--features telemetry`; the script will invoke
  `maturin develop` on the fly if `the_block` is missing.
* `TB_PURGE_LOOP_SECS` defaults to `1`; set another positive integer to
  change the interval. The `demo_runs_clean` test sets it explicitly to `1`,
  forces `PYTHONUNBUFFERED=1`, clears `TB_DEMO_MANUAL_PURGE`, and kills the
  demo if it runs longer than 10 seconds to keep CI reliable while printing
  and preserving demo logs on failure. Set `TB_DEMO_MANUAL_PURGE=1` to opt
  into a manual `ShutdownFlag`/handle example instead of the context manager;
  the README's Quick Start section shows example invocations.

### Tests
* Rust property tests under `tests/test_chain.rs` validate invariants (balances never
  negative, reward decay, duplicate TxID rejection, etc.).
* Fixtures create isolated directories via `tests::util::temp::temp_dir()` and
  clean them automatically after execution so runs remain hermetic.
* `test_replay_attack_prevention` asserts duplicate `(sender, nonce)` pairs are rejected.
* `tests/test_interop.py` confirms Python and Rust encode transactions identically.
* `tests/test_purge_loop_env.py` inserts a TTL-expired transaction and an orphan
  (by deleting the sender) before spawning the loop and asserts
  `ttl_drop_total` and `orphan_sweep_total` each increment while the mempool
  returns to zero.
* `tests/test_spawn_purge_loop.py` spawns two manual purge loops with different
intervals and cross-order joins to assert mempool invariants and metrics.

---

## 20 · Audit & Risk Notes (verbatim)

# Agent/Codex Branch Audit Notes

## Recent Fixes
- Enforced compile-time genesis hash verification and centralized genesis hash computation. **COMPLETED/DONE** [commit: e10b9cb]
- Patched `bootstrap.sh` to install missing build tools and hard-fail on venv mismatches. **COMPLETED/DONE** [commit: e10b9cb]
- Isolated chain state into per-test temp directories and cleaned them on drop;
  replay attack prevention test now asserts duplicate `(sender, nonce)` pairs are
  rejected. **COMPLETED/DONE**
- Added mempool priority comparator unit test proving `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)` ordering. **COMPLETED/DONE**
- Introduced TTL-expiry regression test and telemetry counter `ttl_drop_total`; lock-poison drops now advance `lock_poison_total`.
- Unified mempool critical section (`mempool_mutex → sender_mutex`) covering counter
  updates, heap operations, and pending reservations. Concurrency test
  `flood_mempool_never_over_cap` proves the size cap.
- Orphan sweeps rebuild the heap when `orphan_counter > mempool_size / 2`,
  emit `ORPHAN_SWEEP_TOTAL`, and reset the counter.
- `maybe_spawn_purge_loop` reads `TB_PURGE_LOOP_SECS`/`--mempool-purge-interval`
  and periodically calls `purge_expired`, advancing TTL and orphan-sweep metrics
  even without new transactions.
- Serialized `timestamp_ticks`, rebuilt the mempool on startup, and invoked
  `purge_expired` to drop expired or missing-account entries while logging
  `expired_drop_total` and advancing `ttl_drop_total`.
- **B‑5 Startup TTL Purge — COMPLETED** – `Blockchain::open` batches mempool entries,
  invokes [`purge_expired`](src/lib.rs#L1597-L1666) on startup
  ([src/lib.rs](src/lib.rs#L918-L935)), records `expired_drop_total`, and
  advances `ttl_drop_total` and `startup_ttl_drop_total`.
- Panic-inject eviction test proves rollback and advances lock-poison metrics.
- Completed telemetry coverage: counters `ttl_drop_total`, `orphan_sweep_total`,
  `lock_poison_total`, `invalid_selector_reject_total`,
  `balance_overflow_reject_total`, `drop_not_found_total`, and
  `tx_rejected_total{reason=*}` advance on every rejection; spans
  `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
  `startup_rebuild` record sender, nonce, fee-per-byte, and current
  mempool size ([src/lib.rs](src/lib.rs#L1067-L1082),
  [src/lib.rs](src/lib.rs#L1536-L1542),
  [src/lib.rs](src/lib.rs#L1622-L1657),
  [src/lib.rs](src/lib.rs#L879-L889)). `serve_metrics` scrape example
  documented; `rejection_reasons.rs` asserts the labelled counters and
  `admit_and_mine_never_over_cap` confirms capacity during mining.
- Startup rebuild now processes mempool entries in batches and records
  `startup_ttl_drop_total` (expired mempool entries dropped during startup);
  bench `startup_rebuild` compares batched vs
  naive loops.
- Cached serialized transaction sizes inside `MempoolEntry` so
  `purge_expired` computes fee-per-byte without reserializing;
  `scripts/check_anchors.py --md-anchors` now validates Markdown section
  and Rust line links in CI.
- Introduced Pythonic `PurgeLoop` context manager wrapping `ShutdownFlag`
  and `PurgeLoopHandle`; `demo.py` and docs showcase `with PurgeLoop(bc):`.
- `TTL_DROP_TOTAL` and `ORPHAN_SWEEP_TOTAL` counters saturate at
  `u64::MAX`; tests prove `ShutdownFlag.trigger()` stops the purge loop
  before overflow.
- Added direct `spawn_purge_loop` Python binding, enabling manual
  interval selection, concurrent loops, double trigger/join tests, and
  panic injection via `panic_next_purge`.
- Expanded `scripts/check_anchors.py` to crawl `src`, `tests`, `benches`, and
  `xtask` directories with cached file reads and parallel scanning; updated
  tests cover anchors into `tests/` and `run_all_tests.sh` now warns when
  `jq` or `cargo fuzz` is unavailable, skipping feature detection rather than
  aborting.
- `TxAdmissionError` is `#[repr(u16)]` with stable `ERR_*` constants; Python
  exposes `.code` and telemetry `log_event` entries now carry a numeric
  `code` field alongside `reason`.
- Property-based `fee_recompute_prop` test randomizes blocks, coinbases, and
  fees to ensure migrations recompute emission totals and `fee_checksum`
  correctly; `test_schema_upgrade_compatibility` asserts coinbase sums and
  per-block fee hashes for legacy fixtures.
- Archived `artifacts/fuzz.log` and `artifacts/migration.log` with accompanying
  `RISK_MEMO.md` capturing residual risk and review requirements.
- Introduced a minimal TCP gossip layer (`src/net`) with peer discovery and
  longest-chain adoption; `tests/net_gossip.rs` spins up three nodes to
  confirm chain-height convergence.
- Added command-line `node` binary with JSON-RPC for balance queries,
  transaction submission, mining control, and metrics export; flags
  `--mempool-purge-interval` and `--metrics-addr` wire into purge loop and
  Prometheus exporter.
- RPC server migrated to `tokio` with async tasks replacing per-connection threads for scalable handling.
- `tests/node_rpc.rs` now performs a JSON-RPC smoke test, hitting the metrics,
  balance, and mining-control endpoints.
- `tests/test_purge_loop_env.py` now inserts both a TTL-expired transaction
  and an orphaned one by deleting its sender, then verifies
  `ttl_drop_total`, `orphan_sweep_total`, and `mempool_size` counters.
- `Blockchain::open`, `mine_block`, and `import_chain` refresh the public
  `difficulty` field using `expected_difficulty`; `tests/difficulty.rs`
  asserts retargeting doubles or halves difficulty for fast/slow blocks.
- Table-driven `test_tx_error_codes.py` covers all `TxAdmissionError` variants
  (including lock-poison) and asserts each exception's `.code` matches its
  `ERR_*` constant; `tests/logging.rs` parses telemetry JSON and confirms
  accepted and duplicate transactions carry numeric `code` fields.
- `tests/demo.rs` spawns the Python demo with a 10-second timeout, sets
  `TB_PURGE_LOOP_SECS=1`, forces unbuffered output, and sets
  `TB_DEMO_MANUAL_PURGE` to the empty string so the manual path stays
  disabled; demo logs print and persist on failure.
- Added `tests/test_spawn_purge_loop.py` concurrency coverage spawning two
  manual loops with different intervals and cross-order joins to prove clean
  shutdown and idempotent handle reuse.
- `mempool_order_invariant` now checks transaction order equality instead of
  block hash to avoid timestamp-driven divergence.
- README documents the `TB_DEMO_MANUAL_PURGE` flag for the manual
  purge-loop demonstration, and `CONSENSUS.md` records the timestamp-based
  difficulty retargeting window (120 blocks, 1 000 ms spacing, clamp
  ¼–×4).

## Outstanding Blockers
- **Replay & Migration Tests**: restart suite now covers TTL expiry, and `test_schema_upgrade_compatibility` verifies v1/v2/v3 → v4 migration.

The following notes catalogue gaps, risks, and corrective directives observed across the current branch. Each item is scoped to the current repository snapshot. Sections correspond to the original milestone specifications. Where applicable, cited line numbers reference this repository at HEAD.

Note: `cargo +nightly clippy --all-targets -- -D warnings` reports style and
documentation issues. Failing it does not change runtime behaviour but leaves
technical debt.

## 1. Nonce Handling and Pending Balance Tracking
- **Sequential Nonce Enforcement**: `submit_transaction` checks `tx.payload.nonce != sender.nonce + sender.pending_nonce + 1` (src/lib.rs, L427‑L428). This enforces strict sequencing but does not guard against race conditions between concurrent submissions. A thread‑safe mempool should lock the account entry during admission to avoid double reservation.
- **Pending Balance Reservation**: Pending fields (`pending.consumer`, `pending.industrial`, `pending.nonce`) increment on admission and decrement only when a block is mined (src/lib.rs, L454‑L456 & L569‑L575). There is no path to release reservations if a transaction is dropped or replaced; a mempool eviction routine must unwind the reservation atomically. **COMPLETED/DONE** [commit: e10b9cb]
  - Added `drop_transaction` API that removes a mempool entry, restores balances, and clears the `(sender, nonce)` lock.
- **Atomicity Guarantees**: The current implementation manipulates multiple pending fields sequentially. A failure mid‑update (e.g., panic between consumer and industrial adjustments) can leave the account in an inconsistent state. Introduce a single struct update or transactional storage operation to guarantee atomicity.
- **Mempool Admission Race**: Because `mempool_set` is queried before account mutation, two identical transactions arriving concurrently could both pass the `contains` check before the first insert. Convert to a `HashSet` guarded by a `Mutex` or switch to `dashmap` with atomic insertion semantics.
- **Sender Lookup Failure**: `submit_transaction` returns “Sender not found” if account is absent, but there is no API surface to create accounts implicitly. Decide whether zero‑balance accounts should be auto‑created or require explicit provisioning; document accordingly.

## 2. Fee Routing and Overflow Safeguards
- **Fee Decomposition (`decompose`)**:
  - The function clamps `fee > MAX_FEE` and supports selectors {0,1,2}. Selector `2` uses `div_ceil` to split odd fees. However, the lack of a `match` guard for selector `>2` in `submit_transaction` means callers bypass `decompose` and insert invalid selectors directly into stored transactions. Admission should reject `tx.payload.fee_selector > 2` before persisting. **COMPLETED/DONE** [commit: e10b9cb]
    - `submit_transaction` now enforces selector bounds and documents `MAX_FEE` with a CONSENSUS.md reference.
- **Miner Credit Accounting**:
  - Fees are credited directly to the miner inside the per‑transaction loop (src/lib.rs, L602‑L608) instead of being aggregated into `coinbase_consumer/industrial` and applied once. This violates the “single credit point” directive and complicates block replay proofs. **COMPLETED/DONE** [commit: e10b9cb]
  - No `u128` accumulator is used; summing many near‑`MAX_FEE` entries could overflow `u64` before the clamp. Introduce `u128` accumulators for `total_fee_ct` and `total_fee_it`, check against `MAX_SUPPLY`, then cast to `u64` after clamping. **COMPLETED/DONE**


---

## 2 · Repository Layout

```text
.
├── src/                  # Rust crate root  (lib + optional bin targets)
│   ├── lib.rs            # Public kernel API (PyO3 + native)
│   ├── blockchain/       # consensus, mining, validation
│   ├── crypto/           # ed25519, blake3, canonical serialization
│   └── utils/            # misc helpers (logging, hex, etc.)
├── tests/                # Rust integration + property tests (proptest)
├── benches/              # Criterion benchmarks
├── demo.py               # End‑to‑end Python demo (kept working forever)
├── bootstrap.sh          # Unix bootstrap (Rust, Python, maturin, clippy, etc.)
├── bootstrap.ps1         # PowerShell bootstrap (Windows)
├── docs/                 # Spec & design docs (rendered by mdBook)
│   └── signatures.md     # Canonical serialization & domain‑separation spec
│   └── detailed_updates.md  # granular change log for auditors
├── .github/workflows/    # CI definitions (GitHub Actions)
└── AGENTS.md             # ← You are here
```

> **Rule:** *If a file type isn’t listed above, ask before introducing it.*

---

## 3 · System Requirements

| Component   | Minimum                                            | Recommended    | Notes                                                              |
| ----------- | -------------------------------------------------- | -------------- | ------------------------------------------------------------------ |
| **Rust**    | 1.74 (2023‑10‑05)                                  | nightly‑latest | `rustup toolchain install` managed via `bootstrap.sh`              |
| **Python**  | 3.12.x                                             | 3.12.x         | Headers/dev‑pkg required for PyO3; managed by `pyenv` in bootstrap |
| **Node**    | 20.x                                               | *optional*     | Only if you build ancillary tooling; bootstrap installs nvm 0.39.7 |
| C toolchain | clang 13 / gcc 11                                  | clang 16       | Need `libclang` for maturin on Windows                             |
| **OS**      | Linux (glibc≥2.34) / macOS 12 / Windows 11 (WSL 2) | Same + Docker  | CI matrix enforces all                                             |

---

## 4 · Bootstrapping & Environment Setup

### TL;DR

```bash
# Unix/macOS
bash ./bootstrap.sh   # idempotent; safe to re‑run anytime

# Windows (PowerShell 7+)
./bootstrap.ps1
```

The script installs/upgrades:

1. **Rust** (`rustup`, correct toolchain, `cargo‑binutils`, `clippy`, `llvm‑tools-preview`).
2. **Python 3.12** via **pyenv** + **virtualenv** at `./.venv`.  Environment variables are pinned in `.env`.
3. **Maturin** for wheel builds (`cargo install maturin`).
4. **Node** (optional) via **nvm** `20.x`.
5. Native build deps (`libssl‑dev`, `pkg‑config`, `clang`, …); script autodetects OS/package‑manager.

On startup the script runs database migrations via `cargo run --bin db_migrate` and compacts the default `chain_db` using `./db_compact.sh`. Re-run `db_compact.sh` manually to verify integrity after crashes or before archival.

All developers must install the repo's `githooks/pre-commit` hook to ensure the virtualenv is active before committing:

```bash
ln -sf ../../githooks/pre-commit .git/hooks/pre-commit
```

### Environment variables

- `TB_PURGE_LOOP_SECS` — positive integer controlling purge-loop cadence; `1` keeps tests fast.
- `PYTHONUNBUFFERED` — set to `1` for deterministic Python logs in demos and tests.
- `TB_DEMO_MANUAL_PURGE` — set to `1` to require an explicit purge-loop shutdown in `demo.py`.

 > **Tip:** After any `.rs` or `Cargo.toml` change, run `maturin develop --release --features telemetry` to rebuild and re‑install the Python module in‑place. The `pytest` harness will call `maturin develop` automatically if `the_block` is missing, but running it yourself keeps the extension fresh during development.

---

## 5 · Build & Install Matrix

| Scenario               | Command                                               | Output                               |
| ---------------------- | ----------------------------------------------------- | ------------------------------------ |
| Rust‑only dev loop     | `cargo test --all`                                    | runs lib + test binaries             |
| PyO3 wheel (manylinux) | `maturin build --release --features extension-module` | `target/wheels/the_block‑*.whl`      |
| In‑place dev install   | `maturin develop --release --features telemetry`     | `import the_block` works in `.venv`  |
| Audit + Clippy         | `cargo clippy --all-targets -- -D warnings`           | zero warnings allowed                |
| Benchmarks             | `cargo bench`                                         | Criterion HTML in `target/criterion` |

> Clippy checks style and potential bugs; failing pedantic lints does not
> affect runtime behaviour but leaves technical debt.
>
> **CI will fail** any PR that leaves `clippy` warnings, `rustfmt` diffs, or
> test failures.
>
> Repository verified lint-clean on 2025-02-14 via `cargo fmt` and
> `cargo clippy --all-targets --all-features -- -D warnings`.

---

## 6 · Testing Strategy

1. **Unit Tests** (`#[cfg(test)]` in each module) — fast, no I/O.
2. **Property Tests** (proptest) — randomized blockchain invariants (`tests/test_chain.rs`).
3. **Cross‑Language Determinism** — Python ↔ Rust serialization byte‑for‑byte equality for 100 random payloads (`tests/test_determinism.py`).
4. **Fuzzing** (`cargo fuzz run verify_sig`) — signature verification stability, 10 k iterations on CI.
5. **Benchmarks** (Criterion) — `verify_signature` must stay < 50 µs median on Apple M2.
6. **Pytest Auto-Build** — `tests/conftest.py` runs `maturin develop` if `import the_block` fails, so Python tests can run without manual setup.
7. **Demo Integration** — `cargo test --release demo_runs_clean` runs `demo.py`; it defaults `TB_PURGE_LOOP_SECS=1` when unset, forces `PYTHONUNBUFFERED=1`, and leaves `TB_DEMO_MANUAL_PURGE` empty; logs are captured on failure. The demo auto-installs `the_block` with `maturin` if the module is missing.

8. **Lock-Poison Helper** — use `poison_mempool(bc)` to simulate a poisoned mutex and exercise `ERR_LOCK_POISON` and `lock_poison_total` paths.

Run all locally via:

```bash
./scripts/run_all_tests.sh   # wrapper calls cargo, pytest, fuzz (quick), benches (optional)
```

### Flaky Tests

`demo_runs_clean` occasionally times out on slow hardware. Ensure `TB_PURGE_LOOP_SECS=1`, `PYTHONUNBUFFERED=1`, and `TB_DEMO_MANUAL_PURGE` is unset; re-run with `-- --nocapture` to capture demo logs if failures persist. If the first run is compiling the Python wheel, execute `python demo.py --max-runtime 1` once to pre-build it before rerunning tests.

---

## 7 · Continuous Integration

CI is GitHub Actions; each push/PR runs **seven** jobs:

1. **Lint** — `cargo fmt -- --check` + `black --check` + `ruff check .` + `python scripts/check_anchors.py --md-anchors`.
   - If `black --check` suggests changes for `scripts/check_anchors.py` or `tests/test_tx_error_codes.py`, run `black scripts/check_anchors.py tests/test_tx_error_codes.py` before rerunning the lint step.
2. **Build Matrix** — Linux/macOS/Windows in debug & release.
3. **Tests** — `cargo test --all --release` + `pytest`.
4. **Cargo Audit** — `cargo audit -q` must report zero vulnerabilities.
5. **Udeps** — `cargo +nightly udeps --all-targets` ensures no unused dependencies.
6. **Fuzz Smoke** — 1 k iterations per target to catch obvious regressions.
7. **Wheel Build** — `maturin build` and `auditwheel show` to confirm manylinux compliance.
8. **Isolation** — each test uses `tests::util::temp::temp_dir` so every
   `Blockchain` instance writes to a fresh temp directory that is removed
   automatically when the handle drops.
9. **Replay Guard** — `test_replay_attack_prevention` exercises the
   `(sender, nonce)` dedup logic; duplicates must be rejected.

Badge status must stay green on `main`.  Failing `main` blocks all merges.

---

## 8 · Coding Standards

### Rust

* Edition 2021, MSRV 1.74.
* `#![forbid(unsafe_code)]` — any `unsafe` requires a security review PR.
* Run `cargo fmt --all` before commit; CI enforces.
* **Clippy:** treat warnings as errors; suppress sparingly with `#[allow(...)]` and link to issue.
* Function length guideline: ≤ 40 LOC; break out helpers.
* Prefer explicit `impl` blocks over blanket `pub use` re‑exports.

### Python

* PEP‑8 via **black** (CI enforced).
* Typing everywhere (`from __future__ import annotations`).
* No global state in demos; use local fixtures.

### Commit Message Convention (Conventional Commits subset)

```
feat: add merkle accumulator for block receipts
fix: handle DB reopen error
refactor(crypto): replace blake2 with blake3
```

Wrap body at 80 cols; explain **why**, not **how**.

---

## 9 · Commit & PR Protocol

1. **Branch** from `main`: `git checkout -b feat/<topic>`.
2. Commit granular changes; `git push` drafts to origin.
3. Open PR; template auto‑populates **Summary**, **Testing**, **Docs Updated?**.
4. Request at least *one* reviewer (code‑owner bot will assign).
5. All status checks **must pass**.  Re‑run CI only if deterministic flakes are proven.
6. **Squash‑merge**; PR title becomes commit, bodies concatenated.

File citation syntax inside PR description/comment:
`F:src/crypto/serialize.rs†L42-L57`.

---

## 10 · Subsystem Specifications

### 10.1 Crypto Layer

* **Hash**: BLAKE3‑256 (`blake3::Hash`).
* **Sig**: Ed25519 (`ed25519‑dalek 2.x`) using **strict** verification.
* **Domain Tag**: `b"THE_BLOCK|v1|<chain_id>|"` prepended to payload bytes.
* **Canonical Encoding**: `bincode 1.3` with:

  * little‑endian
  * fixed‑int encoding
  * reject trailing bytes.

### 10.2 Transaction Model

```rust
pub struct RawTxPayload {
    pub from_: Address,           // 32‑byte (blake3 of pubkey)
    pub to:    Address,
    pub amount_consumer:   u64,
    pub amount_industrial: u64,
    pub fee:   u64,
    pub nonce: u64,
    pub memo:  Vec<u8>,           // optional, max 140 bytes
}

pub struct SignedTransaction {
    pub payload:    RawTxPayload,
    pub public_key: PublicKey,    // 32 bytes
    pub signature:  Signature,    // 64 bytes (Ed25519)
}
```

* `tx.id()` = `blake3(b"TX"|serialize(payload)|public_key)`.
* Transactions **must** verify sig, fee, balance, nonce before entering mempool.
* A minimum `fee_per_byte` of `1` is enforced; lower fees are rejected.

### 10.3 Consensus & Mining — Operational Spec

**Proof-of-Work (PoW)**

* Algorithm: BLAKE3, little-endian target; 256-bit full-width comparison.
* Retarget: Weighted-moving-average over last 120 blocks; clamp ΔD ∈ [¼, ×4].
* Block cadence: τ = 1 s target; orphan rate ≤ 1 %.
* Nonce partitioning: core-indexed bit-striping (`nonce = (core_id<<56)|counter`) guarantees deterministic traversal across compilers.

**Proof-of-Service (PoSₑᵣᵥ)**

* Embedded “resource-receipt” commitment in block N validated at N + k; slash on non-delivery.
* Weight merges with PoW for fork-choice via additive work metric `W = Σ(PoW + PoSₑᵣᵥ)`.

**Dual-Token Coinbase**

* Fields: `coinbase_consumer`, `coinbase_industrial` (`TokenAmount`).
* Emission law: `Rₙ = ⌊R₀·ρⁿ⌋`, `ρ = 0.99995`, hard-cap 20 T per token.
* Fee routing strictly follows `ν ∈ {0,1,2}` matrix (consumer, industrial, split) and is provably supply-neutral.

**Validation pipeline (must short-circuit on first failure)**

1. Header sanity & `schema_version`.
2. Difficulty target met (`leading_zero_bits ≥ difficulty`).
3. `calculate_hash()` recomputation matches `header.hash`.
4. Coinbase correctness: `header` ↔ `tx[0]`.
5. Merkle (pending) / tx ID de-dup.
6. Per-tx: signature → stateless (nonce, fee) → stateful (balance).

Fail fast, log once, halt node.

### 10.4 Governance & Upgrade Path

See `docs/governance.md` for badge-weighted voting and fork activation
procedures. Every proposed upgrade must ship a JSON feature flag under
`governance/feature_flags/` declaring activation height and protocol version.

## 11 · Security & Cryptography — Red-Team Grade Controls <a id="11-security--cryptography"></a>

| Threat | Hard Control (compile-time) | Soft Control (run-time) | Audit Evidence |
| ------ | --------------------------- | ----------------------- | -------------- |
| Replay across networks | Chain-ID + domain-tag baked into every signed byte; version byte in tx-id. | Genesis hash pinned in config. | Cross-lang test-vector `replay_guard.bin`. |
| Signature malleability | `ed25519-dalek::Verifier::verify_strict`; rejects non-canonical S. | Batch-verify leverages cofactor-cleared keys. | Fuzz harness `sig_malleate.rs` must find 0 false positives. |
| Serialization drift | Single CFG instance behind `once_cell`; compile-fail on accidental default bincode. | CI diff of 1 000 random payloads Rust ↔ Python. | Report artefact `ser_equivalence.csv`. |
| DB corruption / torn write | `sled` checksum = true; all writes in atomic batch. | On-startup snapshot hash compared to tip header. | Crash-recovery test `db_kill.rs`. |
| Unsafe code | `#![forbid(unsafe_code)]` across workspace. | None—compile-time absolute. | `cargo geiger` score = 0. |
| Crypto dependency compromise | `Cargo.lock` hash-pinned; upgrades only via “crypto-upgrade” PR with two-party review + bench diff. | Run-time signature self-test on launch. | Upgrade checklist in `SECURITY.md`. |

Emergent-threat protocol: 4-hour SLA from CVE disclosure → hot-patch published; nodes auto-reject outdated peers via feature-bit handshake.

## 12 · Persistence & State — Durability Contract

Storage engine: `sled` (log-structured, crash-safe); all column-families prefixed (`b"chain:", b"state:", b"emission:").`

Atomicity: every block commit executed via `Db::transaction(|t| { … })`; guarantees header + state delta cannot diverge.

Write-ahead log cadence: configurable `flush_interval` in blocks (default = 1 main-net, 0 for tests).

### Snapshot/Restore

`scripts/db_snapshot.sh <path>` ⇒ compressed `.blkdb.gz` plus SHA-256 manifest.

`--restore <file>` rehydrates DB, replays WAL, and cross-checks root hash.

CI restores latest snapshot nightly; mismatch → block merge.

### Schema evolution

Monotonic `schema_version` (`u32`) stored per-DB; migrations expressed as pure `(vN)->(vN+1)` functions and executed in temp DB before swap. No in-place mutation.

### State Merklisation (roadmap)

Account trie root every 2¹⁰ blocks; light-client proof ≤ 512 B for 1 M accounts.

## 13 · Troubleshooting Playbook

| Symptom                            | Likely Cause                         | Fix                                                              |
| ---------------------------------- | ------------------------------------ | ---------------------------------------------------------------- |
| `ModuleNotFoundError: the_block`   | Wheel built but not installed        | `maturin develop --release --features telemetry`                                      |
| `libpython3.12.so` linked in wheel | Forgot `--features extension-module` | Re‑build wheel with flag or make feature default in `Cargo.toml` |
| Same tx hash repeats               | `nonce` missing / fake sig           | Ensure unique nonce & real signature                             |
| `cargo test` fails on CI only      | Missing system pkg                   | Check GitHub matrix log; patch `bootstrap.sh`                    |
| `demo_runs_clean` hangs or times out | Purge loop thread not shutting down or env vars missing | Run with `TB_PURGE_LOOP_SECS=1`, `PYTHONUNBUFFERED=1`, and unset `TB_DEMO_MANUAL_PURGE`; inspect persisted stdout/stderr |

If the playbook fails, open an issue with *exact* cmd + full output.

---

## 14 · Glossary & References

* **Address** — BLAKE3‑256 digest of pubkey; displayed as `hex::<32>`.
* **Domain Separation** — Prefixing crypto input with context string to avoid cross‑protocol replay.
* **MSRV** — *Minimum Supported Rust Version*.
* **PoW** — Proof of Work; see `docs/consensus.md`.
* **PyO3** — Rust ↔ Python FFI layer enabling native extension modules.
* **SimpleDb** — in-memory key-value map powering prototype state management.

Further reading: `docs/consensus.md`, `docs/signatures.md`, and `/design/whitepaper.pdf` (WIP).

---

## 15 · Outstanding Blockers & Directives

The mempool, persistence, and observability subsystems remain partially implemented. The following items are **mandatory** before
any testnet or production exposure. Each change **must** include tests, telemetry, and matching documentation updates.

### B‑1 · Global Mempool Mutex — **COMPLETED**
- `submit_transaction`, `drop_transaction`, and `mine_block` enter a
  `mempool_mutex → sender_mutex` critical section with counter updates,
  heap ops, and pending reservations inside. Regression test
  `flood_mempool_never_over_cap` proves the cap.

### B‑2 · Orphan Sweep & Heap Rebuild — **COMPLETED**
- An `orphan_counter` triggers a heap rebuild when
  `orphan_counter > mempool_size / 2`. TTL purges and explicit drop
  operations decrement the counter; when the ratio hits >50 %, a sweep
  rebuilds the heap, drops all orphaned entries, emits
  `ORPHAN_SWEEP_TOTAL`, and resets the counter.

### B‑3 · Timestamp Persistence — **COMPLETED**
- Serialize `MempoolEntry.timestamp_ticks` in schema v4 and rebuild the heap during `Blockchain::open`.
- Drop expired or missing-account entries on startup, logging `expired_drop_total`.
- Update `CONSENSUS.md` with encoding details and migration notes.

### B‑4 · Eviction Deadlock Proof — **COMPLETED**
- Provide a panic‑inject test that forces eviction mid‑admission to demonstrate lock ordering and full rollback.
- Record `LOCK_POISON_TOTAL` and rejection reasons on every failure path.

### B‑5 · Startup TTL Purge — **COMPLETED**
- Ensure `purge_expired()` runs during `Blockchain::open` and is covered by a restart test that proves `ttl_drop_total` advances.
- Spec the startup purge behaviour and default telemetry in `CONSENSUS.md` and docs.

### B‑6 · Dynamic Difficulty Retargeting — **COMPLETED**
- Implement `expected_difficulty` using a moving average over the last ~120
  block timestamps with the adjustment bounded to the range \[¼, ×4].
- Include `difficulty` in block headers; `mine_block` encodes it and
  `validate_block` rejects mismatches.

### B‑7 · Cross-Language Determinism — **COMPLETED**
- Expose `decode_payload` through FFI and add a Rust ↔ Python
  round‑trip test (`serialization_equiv.rs` and
  `scripts/serialization_equiv.py`) wired into CI.

### B‑8 · Purge Loop Controls — **COMPLETED**
- Expose `ShutdownFlag`, `PurgeLoopHandle`, and a Pythonic `PurgeLoop`
  context manager that spawns the purge loop on `__enter__` and triggers
  shutdown on `__exit__`; `maybe_spawn_purge_loop` remains available for
  manual control.
- Bind `spawn_purge_loop` directly to Python to allow explicit
  interval selection and concurrent loop testing; dropping the returned
  handle or triggering its `ShutdownFlag` stops the thread.
- `TTL_DROP_TOTAL` and `ORPHAN_SWEEP_TOTAL` counters saturate at
  `u64::MAX`, and tests assert `ShutdownFlag.trigger()` halts the thread
  before further increments.
- Stress tests in `tests/test_spawn_purge_loop.py` run overlapping
  purge loops, log their start/stop times, and assert `mempool_size` and
  metrics after each join to surface race conditions.

### Deterministic Eviction & Replay Safety
- Unit‑test the priority comparator `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)` and prove ordering stability.
- Replay suite includes `ttl_expired_purged_on_restart` for TTL expiry and `test_schema_upgrade_compatibility` verifying v1/v2/v3 disks migrate to v4, hydrating `timestamp_ticks`.

### Telemetry & Logging
- Counters `TTL_DROP_TOTAL` and `ORPHAN_SWEEP_TOTAL` saturate at
  `u64::MAX` to prevent overflow; `STARTUP_TTL_DROP_TOTAL`,
  `LOCK_POISON_TOTAL`, `INVALID_SELECTOR_REJECT_TOTAL`,
  `BALANCE_OVERFLOW_REJECT_TOTAL`, `DROP_NOT_FOUND_TOTAL`, and
  `TX_REJECTED_TOTAL{reason=*}` track all rejection paths. Each
  `TxAdmissionError` variant is `#[repr(u16)]` with a stable `ERR_*`
  constant; `log_event` emits the numeric `code` alongside `reason` and
  Python exceptions expose `.code` for programmatic inspection.
- Instrument spans `mempool_mutex`, `eviction_sweep`, and
  `startup_rebuild` capturing sender, nonce, fee_per_byte, and mempool
  size.
- Document a `curl` scrape example for `serve_metrics` output in
  `docs/detailed_updates.md` and keep `rejection_reasons.rs` exercising
  the labelled counters.

### Networking & Control Surface — COMPLETED
- `net` module exposes a TCP gossip `Node`, thread-safe `PeerSet`, and bincode
  `Message` enums that broadcast transactions and blocks and adopt the
  longest-chain rule.
- `src/bin/node.rs` provides a JSON-RPC server with `--rpc-addr`,
- `--mempool-purge-interval`, and `--metrics-addr` flags for balance queries,
  transaction submission, mining control, and metrics export.
- Integration tests `tests/net_gossip.rs` and `tests/node_rpc.rs` prove gossip
  convergence and exercise the RPC surface end-to-end. Gossip tests cover a
  three-node mesh, a partition/rejoin scenario where an isolated node returns
  with a longer fork, and negative cases where malformed transactions or blocks
  are ignored. They bind to fixed `127.0.0.1:700*` ports and run serially to
  avoid conflicts with other services.
- RPC server returns JSON-RPC–compliant errors for malformed requests or
  unknown methods, and `rpc_concurrent_controls` exercises concurrent mining
  and submission calls to guard against race conditions.

#### RPC session walkthrough

```bash
# Start a node with RPC enabled
cargo run --bin node -- run --rpc-addr 127.0.0.1:3030 --mempool-purge-interval 5

# Generate a key and capture its address
cargo run --bin node -- generate-key alice
ADDR=$(cargo run --bin node -- show-address alice)

# Sign and submit a self-transfer
TX=$(cargo run --bin node -- sign-tx alice '{"from_":"'$ADDR'","to":"'$ADDR'","amount_consumer":1,"amount_industrial":0,"fee":1,"fee_selector":0,"nonce":1,"memo":[]}')
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"submit_tx","params":{"tx":"'$TX'"}}'

# Mine one block and query the new balance
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"start_mining","params":{"miner":"'$ADDR'"}}'
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"stop_mining"}'
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":4,"method":"balance","params":{"address":"'$ADDR'"}}'
```

### Test & Fuzz Matrix
- Property test: inject panics at each admission step to verify reservation rollback and heap invariants.
- 32‑thread fuzz harness: random fees and nonces for ≥10 k iterations asserting capacity and per-account uniqueness.
- Heap orphan stress test: exceed the orphan threshold, trigger rebuild, and assert ordering and metrics.

- Mirror these directives in §§19, 18, and 20.
- Keep `CHANGELOG.md` and §21 (API Changelog) synchronized with new
  errors, metrics, and flags.
- `scripts/check_anchors.py --md-anchors` validates Markdown headings and
  Rust line anchors; CI rejects any broken link.

---

See [README.md#disclaimer](README.md#disclaimer) for project disclaimer and licensing terms.

**Remember:** *Every line of code must be explainable by a corresponding line in this document or the linked specs.* If not, write the spec first.

---

## Appendix · Full AUDIT_NOTES.md (verbatim) <a id="audit-appendix"></a>

# Agent/Codex Branch Audit Notes

## Recent Fixes
- Enforced compile-time genesis hash verification and centralized genesis hash computation. **COMPLETED/DONE** [commit: e10b9cb]
- Patched `bootstrap.sh` to install missing build tools and hard-fail on venv mismatches. **COMPLETED/DONE** [commit: e10b9cb]
- Isolated chain state into per-test temp directories and cleaned them on drop;
  replay attack prevention test now asserts duplicate `(sender, nonce)` pairs are
  rejected. **COMPLETED/DONE**
- Added mempool priority comparator unit test proving `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)` ordering. **COMPLETED/DONE**
- Introduced TTL-expiry regression test and telemetry counter `ttl_drop_total`; lock-poison drops now advance `lock_poison_total`.
- Unified mempool critical section (`mempool_mutex → sender_mutex`) covering counter
  updates, heap operations, and pending reservations. Concurrency test
  `flood_mempool_never_over_cap` proves the size cap.
- Orphan sweeps rebuild the heap when `orphan_counter > mempool_size / 2`,
  emit `ORPHAN_SWEEP_TOTAL`, and reset the counter.
- `maybe_spawn_purge_loop` reads `TB_PURGE_LOOP_SECS`/`--mempool-purge-interval`
  and periodically calls `purge_expired`, advancing TTL and orphan-sweep metrics
  even without new transactions.
- Serialized `timestamp_ticks`, rebuilt the mempool on startup, and invoked
  `purge_expired` to drop expired or missing-account entries while logging
  `expired_drop_total` and advancing `ttl_drop_total`.
- **B‑5 Startup TTL Purge — COMPLETED** – `Blockchain::open` batches mempool entries,
  invokes [`purge_expired`](src/lib.rs#L1597-L1666) on startup
  ([src/lib.rs](src/lib.rs#L918-L935)), records `expired_drop_total`, and
  advances `ttl_drop_total` and `startup_ttl_drop_total`.
- Panic-inject eviction test proves rollback and advances lock-poison metrics.
- Completed telemetry coverage: counters `ttl_drop_total`, `orphan_sweep_total`,
  `lock_poison_total`, `invalid_selector_reject_total`,
  `balance_overflow_reject_total`, `drop_not_found_total`, and
  `tx_rejected_total{reason=*}` advance on every rejection; spans
  `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
  `startup_rebuild` record sender, nonce, fee-per-byte, and current
  mempool size ([src/lib.rs](src/lib.rs#L1067-L1082),
  [src/lib.rs](src/lib.rs#L1536-L1542),
  [src/lib.rs](src/lib.rs#L1622-L1657),
  [src/lib.rs](src/lib.rs#L879-L889)). `serve_metrics` scrape example
  documented; `rejection_reasons.rs` asserts the labelled counters and
  `admit_and_mine_never_over_cap` confirms capacity during mining.
- Startup rebuild now processes mempool entries in batches and records
  `startup_ttl_drop_total` (expired mempool entries dropped during startup);
  bench `startup_rebuild` compares batched vs
  naive loops.
- Cached serialized transaction sizes inside `MempoolEntry` so
  `purge_expired` computes fee-per-byte without reserializing;
  `scripts/check_anchors.py --md-anchors` now validates Markdown section
  and Rust line links in CI.
- Introduced Pythonic `PurgeLoop` context manager wrapping `ShutdownFlag`
  and `PurgeLoopHandle`; `demo.py` and docs showcase `with PurgeLoop(bc):`.
- `TTL_DROP_TOTAL` and `ORPHAN_SWEEP_TOTAL` counters saturate at
  `u64::MAX`; tests prove `ShutdownFlag.trigger()` stops the purge loop
  before overflow.
- Added direct `spawn_purge_loop` Python binding, enabling manual
  interval selection, concurrent loops, double trigger/join tests, and
  panic injection via `panic_next_purge`.
- Expanded `scripts/check_anchors.py` to crawl `src`, `tests`, `benches`, and
  `xtask` directories with cached file reads and parallel scanning; updated
  tests cover anchors into `tests/` and `run_all_tests.sh` now warns when
  `jq` or `cargo fuzz` is unavailable, skipping feature detection rather than
  aborting.
- `TxAdmissionError` is `#[repr(u16)]` with stable `ERR_*` constants; Python
  exposes `.code` and telemetry `log_event` entries now carry a numeric
  `code` field alongside `reason`.
- Property-based `fee_recompute_prop` test randomizes blocks, coinbases, and
  fees to ensure migrations recompute emission totals and `fee_checksum`
  correctly; `test_schema_upgrade_compatibility` asserts coinbase sums and
  per-block fee hashes for legacy fixtures.
- Archived `artifacts/fuzz.log` and `artifacts/migration.log` with accompanying
  `RISK_MEMO.md` capturing residual risk and review requirements.
- Introduced a minimal TCP gossip layer (`src/net`) with peer discovery and
  longest-chain adoption; `tests/net_gossip.rs` spins up three nodes to
  confirm chain-height convergence.
- Added command-line `node` binary with JSON-RPC for balance queries,
  transaction submission, mining control, and metrics export; flags
  `--mempool-purge-interval` and `--metrics-addr` wire into purge loop and
  Prometheus exporter.
- RPC server migrated to `tokio` with async tasks replacing per-connection threads for scalable handling.
- `tests/node_rpc.rs` now performs a JSON-RPC smoke test, hitting the metrics,
  balance, and mining-control endpoints.
- `tests/test_purge_loop_env.py` now inserts both a TTL-expired transaction
  and an orphaned one by deleting its sender, then verifies
  `ttl_drop_total`, `orphan_sweep_total`, and `mempool_size` counters.
- `Blockchain::open`, `mine_block`, and `import_chain` refresh the public
  `difficulty` field using `expected_difficulty`; `tests/difficulty.rs`
  asserts retargeting doubles or halves difficulty for fast/slow blocks.
- Table-driven `test_tx_error_codes.py` covers all `TxAdmissionError` variants
  (including lock-poison) and asserts each exception's `.code` matches its
  `ERR_*` constant; `tests/logging.rs` parses telemetry JSON and confirms
  accepted and duplicate transactions carry numeric `code` fields.
- `tests/demo.rs` spawns the Python demo with a 10-second timeout, sets
  `TB_PURGE_LOOP_SECS=1`, forces unbuffered output, and sets
  `TB_DEMO_MANUAL_PURGE` to the empty string so the manual path stays
  disabled; demo logs print and persist on failure.
- Added `tests/test_spawn_purge_loop.py` concurrency coverage spawning two
  manual loops with different intervals and cross-order joins to prove clean
  shutdown and idempotent handle reuse.
- `mempool_order_invariant` now checks transaction order equality instead of
  block hash to avoid timestamp-driven divergence.
- README documents the `TB_DEMO_MANUAL_PURGE` flag for the manual
  purge-loop demonstration, and `CONSENSUS.md` records the timestamp-based
  difficulty retargeting window (120 blocks, 1 000 ms spacing, clamp
  ¼–×4).

## Outstanding Blockers
- **Replay & Migration Tests**: restart suite now covers TTL expiry, and `test_schema_upgrade_compatibility` verifies v1/v2/v3 → v4 migration.

The following notes catalogue gaps, risks, and corrective directives observed across the current branch. Each item is scoped to the current repository snapshot. Sections correspond to the original milestone specifications. Where applicable, cited line numbers reference this repository at HEAD.

Note: `cargo +nightly clippy --all-targets -- -D warnings` reports style and
documentation issues. Failing it does not change runtime behaviour but leaves
technical debt.

## 1. Nonce Handling and Pending Balance Tracking
- **Sequential Nonce Enforcement**: `submit_transaction` checks `tx.payload.nonce != sender.nonce + sender.pending_nonce + 1` (src/lib.rs, L427‑L428). This enforces strict sequencing but does not guard against race conditions between concurrent submissions. A thread‑safe mempool should lock the account entry during admission to avoid double reservation.
- **Pending Balance Reservation**: Pending fields (`pending.consumer`, `pending.industrial`, `pending.nonce`) increment on admission and decrement only when a block is mined (src/lib.rs, L454‑L456 & L569‑L575). There is no path to release reservations if a transaction is dropped or replaced; a mempool eviction routine must unwind the reservation atomically. **COMPLETED/DONE** [commit: e10b9cb]
  - Added `drop_transaction` API that removes a mempool entry, restores balances, and clears the `(sender, nonce)` lock.
- **Atomicity Guarantees**: The current implementation manipulates multiple pending fields sequentially. A failure mid‑update (e.g., panic between consumer and industrial adjustments) can leave the account in an inconsistent state. Introduce a single struct update or transactional storage operation to guarantee atomicity.
- **Mempool Admission Race**: Because `mempool_set` is queried before account mutation, two identical transactions arriving concurrently could both pass the `contains` check before the first insert. Convert to a `HashSet` guarded by a `Mutex` or switch to `dashmap` with atomic insertion semantics.
- **Sender Lookup Failure**: `submit_transaction` returns “Sender not found” if account is absent, but there is no API surface to create accounts implicitly. Decide whether zero‑balance accounts should be auto‑created or require explicit provisioning; document accordingly.

## 2. Fee Routing and Overflow Safeguards
- **Fee Decomposition (`decompose`)**:
  - The function clamps `fee > MAX_FEE` and supports selectors {0,1,2}. Selector `2` uses `div_ceil` to split odd fees. However, the lack of a `match` guard for selector `>2` in `submit_transaction` means callers bypass `decompose` and insert invalid selectors directly into stored transactions. Admission should reject `tx.payload.fee_selector > 2` before persisting. **COMPLETED/DONE** [commit: e10b9cb]
    - `submit_transaction` now enforces selector bounds and documents `MAX_FEE` with a CONSENSUS.md reference.
- **Miner Credit Accounting**:
  - Fees are credited directly to the miner inside the per‑transaction loop (src/lib.rs, L602‑L608) instead of being aggregated into `coinbase_consumer/industrial` and applied once. This violates the “single credit point” directive and complicates block replay proofs. **COMPLETED/DONE** [commit: e10b9cb]
  - No `u128` accumulator is used; summing many near‑`MAX_FEE` entries could overflow `u64` before the clamp. Introduce `u128` accumulators for `total_fee_ct` and `total_fee_it`, check against `MAX_SUPPLY`, then cast to `u64` after clamping. **COMPLETED/DONE**

## 19. Commit History Review Highlights
- The last commit removed leftover badge artifacts, but a systematic audit should validate no SVG or badge workflows remain in submodules or documentation.
- Merge commits (`Fee Routing v2` and `Full-Lifecycle Hardening`) group broad changes; future work should split features into smaller, auditable commits to simplify bisecting and review.
- Early history contains large binary blobs (`pixi.lock` with thousands of lines). A repository rewrite to purge these from git history would reduce clone time and improve auditability.

## 20. Repository Hygiene
- `analysis.txt` and other scratch files live at repo root; convert such documents into tracked design notes under `docs/` or remove them to avoid confusion.
- Ensure every script in `scripts/` has `set -euo pipefail` and consistent shebangs; `scripts/run_all_tests.sh` now guards against missing `jq` and `cargo-fuzz`, warning and continuing instead of aborting.
- Add `.editorconfig` to enforce consistent indentation (spaces vs tabs) across Rust, Python, and Markdown sources.

## 21. Testing Infrastructure Gaps
- `tests/test_interop.py` imports `the_block` but lacks assertions around fee decomposition or nonce tracking; extend to cover new consensus rules.
- No integration test spans multiple blocks with split fees (`ν=2`) to prove `INV-FEE-01` over several rounds. Create a randomized ledger test mining ≥100 blocks with mixed selectors.
- Property tests do not seed randomness deterministically (`proptest` with `test_runner.config()`); add explicit seeds so failures reproduce reliably in CI.

## 22. Security Considerations
- Absence of rate limiting on `submit_transaction` exposes the node to CPU-bound DoS. Implement token-bucket or fee-based rate limiting.
- Signature verification uses `ed25519-dalek` but does not enforce batch verification or prehashing; consider `ed25519-zebra` for constant-time operations and add tests for signature malleability regression.
- The `chain_id_py()` function returns a constant with no network isolation. Until P2P is implemented, document the risk of cross-environment replays and consider namespacing test networks via configuration.

## 23. Future-Proofing and Design Debt
- `TokenAmount` uses `u64`; migrating to `u128` later may break serialized formats despite wrapper. Define big-endian encoding or explicit versioning to ease transition.
- Difficulty retargeting (mid-term milestone) will require storing per-block timestamps. Current `Block` struct lacks a timestamp field; adding it now simplifies future upgrades.
- No abstraction over persistence layer yet; start by defining a `Storage` trait to decouple the in-memory stub from future backends.

## 24. Documentation Cross-References
- `docs/detailed_updates.md` is referenced in code comments (e.g., `calculate_hash`), but file does not exist in repo. Either add the document or update comments to point to existing specs.
- Glossary terms defined in ECONOMICS.md should be backlinked from AGENTS.md §14 to maintain a single authoritative glossary.
- CHANGELOG entries for fee routing lack PR numbers and commit hashes; include them for traceability.

## 25. Build & Release Process
- No release automation or tagging strategy is defined. Introduce `cargo-release` or similar tooling with pre-tag lint/test hooks.
- Wheel builds rely on manual `maturin develop`. Add a `Makefile` or `justfile` encapsulating build/test steps to avoid command drift across agents.
- Ensure artifacts (`fee_v2.schema.json`, audit reports) are published as GitHub release assets for reproducibility.

## 26. External Dependencies
- `Cargo.toml` pins versions but lacks `cargo deny` configuration to track license and security advisories. Set up `cargo deny` with a denylist/allowlist policy.
- `requirements.txt` includes `maturin` and `pytest` but not exact hashes or versions; use `pip-tools` or `uv` to generate a lock file with hashes to prevent supply-chain drift.
- Node tooling is optional but `package.json` is empty; remove it or populate with meaningful scripts to avoid confusion.

## 27. Developer Experience
- Pre-commit hooks enforce venv activation but do not run `cargo fmt` or `ruff` automatically. Integrate these checks to catch style issues before commit.
- Provide a `devcontainer.json` for VS Code users to standardize environment setup, reducing onboarding friction.
- Add a `CONTRIBUTING.md` section on how to run fuzz tests and migrate databases locally to encourage thorough review by external contributors.

## 28. Future Work Tracking
- Create GitHub issues or a roadmap markdown referencing each audit item to assign ownership and track progress. Embed links from this document to the issues once created.
- Set up milestones corresponding to the long-term vision (P2P networking, storage abstraction, governance) to visualize dependency chains.
- Establish a recurring “audit sync” meeting or asynchronous report so contributors regularly update status against this checklist.

---

## 29. Networking Readiness (Mid-Term Milestone)
- No `p2p` module or dependency exists; begin by selecting a networking crate (`libp2p` or `quic`). Draft message schemas for `TxBroadcast`, `BlockAnnounce`, and `ChainRequest`.
- Design peer handshake including feature bits (`0x0004` for FEE_ROUTING_V2`) and schema version negotiation. Stub out structs so future integration does not disturb current consensus code.
- Plan for mempool synchronization: implement gossip with inventory (`inv`) and getdata style flows to prevent duplicate downloads and enable relay suppression.

## 30. Formal Verification Scaffold
- Repository lacks `formal/` directory promised for F★ specs. Create `formal/fee_v2.fst` stubs mirroring the algebra in ECONOMICS.md with type definitions for `FeeSelector`, `FeeDecomp`, and lemmas (`fee_split_sum`, `inv_fee_01`).
- Provide build tooling (`Makefile` or `fstar.mk`) so CI can check F★ files for syntax and type errors even before proofs are completed.
- Document how invariants map to code modules to guide the formal methods team; e.g., `fee::decompose` ↔ `Fstar.Fee.decompose`.

## 31. Summary of Missing Deliverables
- Governance artefacts (`governance/FORK-FEE-01.json`) — absent.
- P2P feature-bit handshake — absent.
- CI jobs (`fee-unit-tests`, `fee-fuzz-san`, `schema-lint`) — absent.
- Migration test (`test_schema_upgrade_compatibility`) — active.
- Runtime overflow guards for miner credit — incomplete.
- Documentation cross-links and disclaimer updates — incomplete.
- Fuzz harnesses for `admission_check`, `apply_fee`, `validate_block` — missing.
- Grafana dashboards and risk memo — missing.

---

## 32. Risk Register and Stakeholder Assignments
- Establish a `docs/risk_register.md` tracking each economic and technical risk identified here, owner assignment, mitigation status, and review date.
- Assign Lead Economist to validate invariants and fee algebra; Security Chair to sign off on overflow and nonce logic; QA Lead to monitor fuzz dashboards and CI.
- Schedule pre-fork sign-off meeting and record minutes to satisfy governance and audit requirements.

## 33. Concluding Directive
Every item above is a blocker for a production-grade release. Treat this document as a living specification: update it whenever an issue is resolved, add commit references, and ensure future contributors can trace every consensus change to a documented rationale.

---

## 21 · API Changelog (verbatim)

# API Change Log

## Unreleased

### Python
- Python helper `mine_block(txs)` mines a block from signed transactions for quick demos ([src/lib.rs](src/lib.rs)).
- `RawTxPayload` exposes both `from_` and `from` attributes so decoded payloads are accessible via either name ([src/transaction.rs](src/transaction.rs)).
- `TxAdmissionError::LockPoisoned` is returned when a mempool mutex guard is poisoned.
- `TxAdmissionError::PendingLimit` indicates the per-account pending cap was reached.
- `TxAdmissionError::NonceGap` surfaces as `ErrNonceGap` when a nonce skips the expected sequence.
- `TxAdmissionError` instances expose a stable `code` property and constants
  `ERR_*` map each rejection reason to a numeric identifier.
- `decode_payload(bytes)` decodes canonical payload bytes back into `RawTxPayload`.
- `ShutdownFlag` and `PurgeLoopHandle` manage purge threads when used with
  `maybe_spawn_purge_loop`.
- `PurgeLoop(bc)` context manager spawns the purge loop and triggers
  shutdown on exit.
- `maybe_spawn_purge_loop(bc, shutdown)` reads `TB_PURGE_LOOP_SECS` and returns
  a `PurgeLoopHandle` that joins the background TTL cleanup thread.
- `maybe_spawn_purge_loop` now errors when `TB_PURGE_LOOP_SECS` is unset,
  non-numeric, or ≤0; the Python wrapper raises ``ValueError`` with the parse
  message.
- `spawn_purge_loop(bc, interval_secs, shutdown)` spawns the purge loop with a
  manually supplied interval.
- `Blockchain::panic_in_admission_after(step)` panics mid-admission for test harnesses;
  `Blockchain::heal_admission()` clears the flag.
- `Blockchain::panic_next_evict()` triggers a panic during the next eviction and
  `Blockchain::heal_mempool()` clears the poisoned mutex.
- `PurgeLoopHandle.join()` raises `RuntimeError` if the purge thread panicked
  and setting `RUST_BACKTRACE=1` appends a Rust backtrace to the panic message.
- Dropping `PurgeLoopHandle` triggers shutdown automatically if
  `ShutdownFlag.trigger()` was not called.

### Telemetry
- `TTL_DROP_TOTAL` counts transactions purged due to TTL expiry.
- `STARTUP_TTL_DROP_TOTAL` reports expired mempool entries dropped during
  startup rebuild.
- `ORPHAN_SWEEP_TOTAL` tracks heap rebuilds triggered by orphan ratios.
- `LOCK_POISON_TOTAL` records mutex poisoning events.
- `INVALID_SELECTOR_REJECT_TOTAL`, `BALANCE_OVERFLOW_REJECT_TOTAL`, and
  `DROP_NOT_FOUND_TOTAL` expose detailed rejection counts.
- `TX_REJECTED_TOTAL{reason=*}` aggregates all rejection reasons.

### Kernel
- `service_badge` module introduces `ServiceBadgeTracker` and `Blockchain::check_badges()` which evaluates uptime every 600 blocks.
- `serve_metrics(addr)` exposes Prometheus text over a lightweight HTTP listener.
- `maybe_spawn_purge_loop` reads `TB_PURGE_LOOP_SECS` and spawns a background
  thread that periodically calls `purge_expired`, advancing
  `ttl_drop_total` and `orphan_sweep_total`.
- JSON telemetry logs now include a numeric `code` alongside `reason` for
  each admission event.
- Spans `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
  `startup_rebuild` record sender, nonce, fee-per-byte, and mempool size
  ([src/lib.rs](src/lib.rs#L1067-L1082),
  [src/lib.rs](src/lib.rs#L1536-L1542),
  [src/lib.rs](src/lib.rs#L1622-L1657),
  [src/lib.rs](src/lib.rs#L879-L889)).
- Documented `mempool_mutex → sender_mutex` lock order and added
  `admit_and_mine_never_over_cap` regression to prove the mempool size
  invariant.
- **B ‑5 Startup TTL Purge — COMPLETED** – `Blockchain::open` now invokes [`purge_expired`](src/lib.rs#L1597-L1666)
  ([src/lib.rs](src/lib.rs#L918-L935)), recording
  `ttl_drop_total`, `startup_ttl_drop_total`, and `expired_drop_total` on restart.
- Cached serialized transaction sizes in `MempoolEntry` so `purge_expired`
  avoids reserializing transactions (internal optimization).

### Node CLI & RPC
- Introduced `node` binary exposing `--rpc-addr`, `--mempool-purge-interval`,
  and `--metrics-addr` flags.
- JSON-RPC methods `balance`, `submit_tx`, `start_mining`, `stop_mining`, and
  `metrics` enable external control of the blockchain.
- RPC server uses `tokio` for asynchronous connection handling, removing the thread-per-connection model.


### Vision in Brief

The-Block is building a one-second Layer 1 that notarizes sub-second
micro-shards. Economics use two tradeable tokens—Consumer and
Industrial—plus non-transferable service credits. Governance assigns one
badge per reliable node and uses shard districts for bicameral votes. The
current kernel already covers dynamic difficulty, dual-token fee routing,
purge loops with telemetry, basic gossip, and a JSON-RPC node. The tasks
below extend this foundation toward persistent storage, secure networking,
and full service-based governance.
See `agents_vision.md` for the full narrative on how these pieces cohere into
a civic-grade public good.

---

## Completed Roadmap Items (Recap)

The following fixes are already in `main`.  Use them as context and do not
re‑implement:

1. **Genesis integrity** verified at compile time; bootstrap script fixed.
2. **Fee refactor** and consensus logic overhaul, including drop_transaction,
   fee routing safeguards, 128‑bit fee accumulators, checksum fields, and
   distinct error codes surfaced via PyO3.
3. **Documentation refresh**: disclaimer relocation, `Agents-Sup.md`, and
   schema guidance.
4. **Temp DB isolation** for tests; `Blockchain::new(path)` creates per-run
   directories and `test_replay_attack_prevention` enforces `(sender, nonce)`
   dedup.
5. **Telemetry expansion**: HTTP metrics exporter, `ttl_drop_total`,
   `startup_ttl_drop_total` (expired mempool entries dropped during startup), `lock_poison_total`, `orphan_sweep_total`,
   `invalid_selector_reject_total`, `balance_overflow_reject_total`,
   `drop_not_found_total`, `tx_rejected_total{reason=*}`, and span coverage
   for `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
   `startup_rebuild` capturing sender, nonce, fee_per_byte, and
  mempool_size ([`src/lib.rs`](src/lib.rs#L1067-L1082),
   [`src/lib.rs`](src/lib.rs#L1536-L1542),
   [`src/lib.rs`](src/lib.rs#L1622-L1657),
   [`src/lib.rs`](src/lib.rs#L879-L889)). Comparator ordering test for
   mempool priority.
   `maybe_spawn_purge_loop` (wrapped by the `PurgeLoop` context manager)
   reads `TB_PURGE_LOOP_SECS`/`--mempool-purge-interval` and calls
   `purge_expired` periodically, advancing TTL and orphan-sweep metrics.
   Setting `TB_DEMO_MANUAL_PURGE=1` while running `demo.py` skips the
   context manager and demonstrates manual `ShutdownFlag` + handle
   control instead.
6. **Mempool atomicity**: global `mempool_mutex → sender_mutex` critical section with
   counter updates, heap ops, and pending balances inside; orphan sweeps rebuild
   the heap when `orphan_counter > mempool_size / 2` and emit `ORPHAN_SWEEP_TOTAL`.
7. **Timestamp persistence & eviction proof**: mempool entries persist
   `timestamp_ticks` for deterministic startup purge; panic-inject eviction test
   proves lock-poison recovery.
8. **B‑5 Startup TTL Purge — COMPLETED**: `Blockchain::open` batches mempool rebuilds,
   invokes [`purge_expired`](src/lib.rs#L1597-L1666) on startup
   ([src/lib.rs](src/lib.rs#L918-L935)), and restart tests ensure both
   `ttl_drop_total` and `startup_ttl_drop_total` advance.
9. Cached each transaction's serialized size in `MempoolEntry` and updated
   `purge_expired` to use the cached fee-per-byte, avoiding reserialization.
   `scripts/check_anchors.py --md-anchors` now validates Rust line and
   Markdown section links in CI.
10. Dynamic difficulty retargeting with per-block `difficulty` field,
    in-block nonce continuity validation, Pythonic `PurgeLoop` context
    manager wrapping `ShutdownFlag` and `PurgeLoopHandle`, and
    cross-language serialization determinism tests. `Blockchain::open`,
    `mine_block`, and `import_chain` now refresh the `difficulty`
    field to the current network target.
11. Telemetry counters `TTL_DROP_TOTAL` and `ORPHAN_SWEEP_TOTAL` saturate at
    `u64::MAX`; tests confirm `ShutdownFlag.trigger()` halts purge threads
    before overflow.
12. Stable transaction-admission error codes: `TxAdmissionError` is
    `#[repr(u16)]`, Python re-exports `ERR_*` constants and `.code` attributes,
    and `log_event` emits the numeric `code` in telemetry JSON. A
    table-driven `tests/test_tx_error_codes.py` enumerates every variant and
    asserts `exc.code == ERR_*`; a doc-hidden `poison_mempool(bc)` helper
    enables lock-poison coverage, while `tests/logging.rs` parses telemetry
    JSON to ensure accepted and duplicate transactions carry the expected
    numeric codes.
13. Anchor checker walks `src`, `tests`, `benches`, and `xtask` with cached
    file reads and parallel scanning; `scripts/test_check_anchors.py` covers
    `tests/` anchors. `run_all_tests.sh` auto-detects optional features via
    `cargo metadata | jq` and warns when `jq` or `cargo fuzz` is absent,
    continuing without them.
14. Direct `spawn_purge_loop(bc, secs, shutdown)` binding for Python enables
    manual interval control and concurrency tests. New tests cover manual
    trigger/join semantics, panic propagation via `panic_next_purge`, and
    env-driven loops sweeping TTL-expired and orphan transactions, asserting
    `ttl_drop_total` and `orphan_sweep_total` each advance and the mempool
    returns to zero.
15. Coinbase/fee recomputation is stress-tested: property-based generator
    randomizes blocks, coinbases, and fees, and schema upgrade tests validate
    emission totals and per-block `fee_checksum` after migration.
16. Minimal TCP gossip layer under `net/` broadcasts transactions and blocks,
    applies a longest-chain rule on conflicts, and has a multi-node
    convergence test.
17. Command-line `node` binary exposes `tokio`-based JSON-RPC endpoints for balance queries,
    transaction submission, mining control, and metrics; `--mempool-purge-interval`
    and `--metrics-addr` flags configure purge loop and Prometheus export.
18. `tests/node_rpc.rs` performs a smoke test of the RPC layer, exercising the
    metrics, balance, and mining-control methods.
19. `demo_runs_clean` integration test injects `TB_PURGE_LOOP_SECS=1`, sets
    `TB_DEMO_MANUAL_PURGE` to the empty string, forces unbuffered Python output,
    enforces a 10-second timeout, and prints demo logs on failure while
    preserving them on disk.
20. `tests/test_spawn_purge_loop.py` spawns two manual purge loops with
    different intervals and cross-order joins, asserting the mempool remains
    stable and that repeated joins are no-ops.
21. README now explains how to opt into the manual purge-loop demo via
    `TB_DEMO_MANUAL_PURGE`, and `CONSENSUS.md` documents the timestamp-based
    difficulty retargeting window (120 blocks, 1 000 ms, clamp ¼–×4).

---

## Current Status & Completion Estimate

*Completion score: **60 / 100*** — dynamic difficulty, nonce continuity, and
Python purge-loop controls elevate the kernel, yet networking and persistent
storage remain open. See
[Project Completion Snapshot](#project-completion-snapshot--60--100) for
detail.

* **Core consensus/state/tx logic** — 94 %: dynamic difficulty retargeting,
  nonce continuity, fee routing, comparator ordering, and global atomicity via
  `mempool_mutex → sender_mutex` are in place.
* **Database/persistence** — 65 %: schema v4 persists admission ticks; durable
  backend pending.
* **Testing/validation** — 70 %: admission and eviction panic tests and
  serialization equivalence checks are in place; long-range fuzzing and
  migration property tests remain.
* **Demo/docs** — 68 %: demo narrates fee selectors and purge-loop lifecycle;
  docs track new metrics but still lack cross-links and startup rebuild
  details.
* **Networking (P2P/sync/forks)** — 10 %: skeleton TCP gossip with peer
  discovery and longest-chain sync landed; full handshake, RPC, and CLI
  remain.
* **Mid-term engineering infra** — 20 %: CI runs fmt/tests and serialization
  determinism, but coverage, fuzzing, schema lint, and contributor automation
  remain.
* **Upgrade/governance** — 0 %: no fork artefacts, snapshot tooling, or
  governance docs.
* **Long-term vision scaffolding** — 0 %: quantum safety, resource proofs,
  layered ledger, and on-chain governance remain conceptual.

**Milestone map**

* `0–40` → R&D, spec, core consensus *(done)*.
* `40–60` → DB, migration, atomicity *(current, >90 % complete)*.
* `60–80` → Networking, CLI/RPC, durable DB, governance stubs.
* `80–100` → Mainnet readiness, testnet burn-in, monitoring, incident tooling.

---

## Immediate Priorities — 0‑2 Months

Treat the following as blockers. Implement each with atomic commits, exhaustive tests, and cross‑referenced documentation.

1. **Admission Atomicity & Ledger Invariants**
   - Use `DashMap::entry` or per-sender mutex to guarantee `(sender, nonce)`
     check+insert and pending-balance reservation happen atomically.
   - Regression tests prove pending balances and nonce sets roll back on panic.
2. **Pending Ledger Consistency Tests**
   - Property tests ensure `pending_consumer + balance.consumer ≥ 0` and
     pending nonces stay contiguous after drops or reorgs.
3. **Persistence Abstraction**
   - Define a `Db` trait and adapt `SimpleDb` to it, paving the way for sled or
     RocksDB implementations.
4. **Networking & CLI Skeleton**
   - Introduce a stub `network` module for block/tx gossip and a simple CLI/RPC
     layer exposing balance queries, transaction submission, mining, and
     metrics.
5. **Documentation & Telemetry**
   - Keep README, AGENTS, and changelogs synchronized; ensure new counters are
     documented and `scripts/check_anchors.py --md-anchors` passes.

---

### Agents-Sup.md (verbatim)

# Agents Supplement — Strategic Roadmap and Orientation

> Authoritative pointer: See AGENTS.md §16–17 for the consolidated, up‑to‑date vision and playbooks. This file remains for context and history.

This document extends `AGENTS.md` with a deep dive into the project's long‑term vision and the immediate development sequence. Read both files in full before contributing.

> Update: This supplement aligns with the unified vision in §16. The “Vision Alignment & Next Steps” section below is authoritative and supersedes older roadmap fragments.

## Vision Alignment & Next Steps (Authoritative)

### People‑Built Internet
- LocalNet (fast road): bonded uplinks, caching, paid relays; strict mobile defaults; receipts and rate‑limits; metrics in Dashboard.
- Range Boost (long road): delay‑tolerant store‑and‑forward; optional lighthouse radios; coverage/delivery earnings; coverage heatmap.
- Carry‑to‑Earn: bundle courier with sealed delivery receipts; commuter routes; privacy explainer.
- Hotspot Exchange: host/guest modes; wrapped traffic; credit meters backed by BLOCKc.
- Neighborhood Update Accelerator: content‑addressed seeding for instant updates/patches.

### Compute Marketplace & CBM
- Shadow intents (stake‑backed) show p25–p75 bands + p_adj. At Industrial TGE, convert escrows to BLOCKi and start two canary lanes (transcode, authenticity). Daily per‑node caps and operator diagnostics.
- Compute‑Backed Money (CBM): daily redeem curves (X BLOCK → Y seconds or Z MB); minimal backstop from marketplace fees; Instant Apps execute via LocalNet and settle later.

### Launch & SDKs
- Consumer‑first TGE: single USDC/BLOCKc pool (1,000,000 : $500), LP time‑lock, 48h slow‑start; publish pool math/addresses. Industrial armed when readiness (nodes/capacity/liquidity/vote) sustains N days.
- SDKs v1: Provenance, Bonded Contact, Commerce, Ownership Card, AI Minutes; sample apps + docs.

### Governance & Legal
- Service‑tied badges; bicameral votes; catalog governance (list/delist logs); treasury streaming.
- Law‑Enforcement Guidelines (metadata‑only); transparency log; jurisdiction modules (client/provider); SBOM/licensing; CLA.
- Founder exit milestones: burn protocol admin keys; reproducible builds; disable privileged RPCs; publish irrevocability txs.

### Deliverables
- Code/tests/metrics/docs as listed in Agent‑Next‑Instructions “Updated Vision & Authoritative Next Steps.”

## 0. Scope Reminder

* **Production Kernel** – The code targets real economic deployment. It is **not** a toy network nor financial instrument.
* **Rust First, Python Friendly** – The kernel is implemented in Rust with PyO3 bindings for scripting and tests. Absolutely no unsafe code is allowed.
* **Dual‑Token Ledger** – Balances are tracked in consumer and industrial units. Token arithmetic uses the `TokenAmount` wrapper.

## 1. Current Architecture Overview

### Consensus & Mining
* Proof of Work using BLAKE3 hashes with dynamic difficulty retargeting.
  `expected_difficulty` computes a moving average over ~120 block timestamps
  clamped to a [¼, ×4] adjustment; headers store the difficulty and validators
  reject mismatches. See [`CONSENSUS.md#difficulty-retargeting`](CONSENSUS.md#difficulty-retargeting)
  for the full algorithm and tuning parameters.
* Each block stores `coinbase_consumer` and `coinbase_industrial`; the first transaction must match these values.
* Block rewards decay by a factor of `DECAY_NUMERATOR / DECAY_DENOMINATOR` each block.

### Accounts & Transactions
* `Account` maintains balances, nonce and pending totals to prevent overspending.
* `RawTxPayload` → `SignedTransaction` using Ed25519 signatures. The canonical signing bytes are `domain_tag || bincode(payload)`.
* Transactions include a `fee_selector` selector (0=consumer, 1=industrial, 2=split) and must use sequential nonces; `validate_block`
  tracks expected nonces per sender and rejects gaps or repeats within a block.

### Storage
* Persistent state lives in an in-memory map (`SimpleDb`). `ChainDisk` encapsulates the
  chain, account map and emission counters. Schema version = 3.
* `Blockchain` tracks its `path` and its `Drop` impl removes the directory.
  `Blockchain::new(path)` expects a unique temp directory; tests use
  `tests::util::temp::temp_dir()` to avoid cross-test leakage and ensure
  automatic cleanup.

### Mempool Concurrency
* A global `mempool_mutex` guards all mempool mutations before the per-sender
  lock. Counter updates, heap pushes/pops, and pending balance/nonces are
  executed inside this lock order, ensuring the invariant `mempool_size ≤
  max_mempool_size`.
* Entries referencing missing accounts increment an `orphan_counter`; once the
  counter exceeds half the mempool, a sweep drops all orphans, emits
  `ORPHAN_SWEEP_TOTAL`, and resets the counter.
* Each mempool entry caches its serialized size so `purge_expired` can compute
  fee-per-byte without reserializing transactions.

### Networking & Gossip
* The `net` module provides a minimal TCP gossip layer with a thread-safe
  `PeerSet` and `Message` enums for `Hello`, `Tx`, `Block`, and `Chain`.
* Nodes broadcast transactions and blocks and adopt longer forks via
  `Blockchain::import_chain`, ensuring convergence on the longest chain.
* `src/bin/node.rs` wraps the chain in a `tokio`-based JSON-RPC server exposing balance queries,
  transaction submission, start/stop mining, and metrics export. Flags
  `--mempool-purge-interval` and `--metrics-addr` configure the purge loop and
  Prometheus endpoint.
* Integration test `tests/net_gossip.rs` spawns three nodes that exchange
  data and verify equal chain heights.
* `tests/node_rpc.rs` smoke-tests the RPC layer by hitting the metrics,
  balance, and mining-control endpoints.

### Telemetry Metrics & Spans
* Metrics: `mempool_size`, `evictions_total`, `fee_floor_reject_total`,
  `dup_tx_reject_total`, `ttl_drop_total`, `startup_ttl_drop_total`
  (expired mempool entries dropped during startup),
  `lock_poison_total`, `orphan_sweep_total`, `invalid_selector_reject_total`,
  `balance_overflow_reject_total`, `drop_not_found_total`,
  `tx_rejected_total{reason=*}`. `ttl_drop_total` and `orphan_sweep_total`
  saturate at `u64::MAX` to avoid overflow.
* `maybe_spawn_purge_loop` reads `TB_PURGE_LOOP_SECS` (or
  `--mempool-purge-interval`) and spawns a thread that periodically calls
  `purge_expired`, advancing TTL and orphan-sweep metrics even when the node is
  idle. Python exposes a `PurgeLoop` context manager wrapping
  `ShutdownFlag`/`PurgeLoopHandle` for automatic startup and clean shutdown;
  manual control is available via the `spawn_purge_loop(bc, secs, shutdown)`
  binding. Set `TB_DEMO_MANUAL_PURGE=1` while running `demo.py` to opt
  into the manual flag/handle demonstration.
* Admission failures are reported with `TxAdmissionError` which is
  `#[repr(u16)]`; Python re-exports `ERR_*` constants and each exception has a
  `.code` attribute. `log_event` includes the same numeric `code` in telemetry
  JSON so log consumers can match on stable identifiers. The
  `tests/test_tx_error_codes.py` suite iterates over every variant to assert
  `exc.code == ERR_*`, a doc-hidden `poison_mempool(bc)` helper enables
  lock-poison coverage, and `tests/logging.rs` captures telemetry JSON for
  admitted transactions, duplicates, nonce gaps, insufficient balances, and
  purge-loop TTL and orphan sweeps.
* Sample JSON logs (`--features telemetry-json`):

  {"op":"reject","sender":"a","nonce":3,"reason":"nonce_gap","code":3}
  {"op":"purge_loop","reason":"ttl_drop_total","code":0,"fpb":1}
  {"op":"purge_loop","reason":"orphan_sweep_total","code":0,"fpb":1}

* Spans: `mempool_mutex` (sender, nonce, fpb, mempool_size),
  `admission_lock` (sender, nonce), `eviction_sweep` (sender, nonce,
  fpb, mempool_size), `startup_rebuild` (sender, nonce, fpb,
  mempool_size). See [`src/lib.rs`](src/lib.rs#L1067-L1082),
  [`src/lib.rs`](src/lib.rs#L1536-L1542),
  [`src/lib.rs`](src/lib.rs#L1622-L1657), and
  [`src/lib.rs`](src/lib.rs#L879-L889).
* `serve_metrics(addr)` exposes Prometheus text; e.g.
  `curl -s localhost:9000/metrics | grep tx_rejected_total`.
  The CLI uses `--metrics-addr` to spawn this exporter during `node run`.

### Schema Migrations & Invariants
* Bump `ChainDisk.schema_version` for any on-disk format change and supply a lossless migration routine with tests.
* Each migration must preserve [`INV-FEE-01`](ECONOMICS.md#inv-fee-01) and [`INV-FEE-02`](ECONOMICS.md#inv-fee-02); update `docs/schema_migrations/` with the new invariants.

### Python Demo
* `demo.py` creates a fresh chain, mines a genesis block, signs a sample
  message, submits a transaction and mines additional blocks while
  printing explanatory output. It uses `with PurgeLoop(bc):` to spawn and
  join the purge thread automatically. Metric assertions require building
  the module with `--features telemetry`; the script will invoke
  `maturin develop` on the fly if `the_block` is missing.
* `TB_PURGE_LOOP_SECS` defaults to `1`; set another positive integer to
  change the interval. The `demo_runs_clean` test sets it explicitly to `1`,
  forces `PYTHONUNBUFFERED=1`, clears `TB_DEMO_MANUAL_PURGE`, and kills the
  demo if it runs longer than 10 seconds to keep CI reliable while printing
  and preserving demo logs on failure. Set `TB_DEMO_MANUAL_PURGE=1` to opt
  into a manual `ShutdownFlag`/handle example instead of the context manager;
  the README's Quick Start section shows example invocations.

### Tests
* Rust property tests under `tests/test_chain.rs` validate invariants (balances never
  negative, reward decay, duplicate TxID rejection, etc.).
* Fixtures create isolated directories via `tests::util::temp::temp_dir()` and
  clean them automatically after execution so runs remain hermetic.
* `test_replay_attack_prevention` asserts duplicate `(sender, nonce)` pairs are rejected.
* `tests/test_interop.py` confirms Python and Rust encode transactions identically.
* `tests/test_purge_loop_env.py` inserts a TTL-expired transaction and an orphan
  (by deleting the sender) before spawning the loop and asserts
  `ttl_drop_total` and `orphan_

## 2. Immediate Next Steps
The following directives are mandatory before any feature expansion. Deliver each with exhaustive tests, telemetry, and cross‑referenced documentation.

1. **B‑3 Timestamp Persistence** — *COMPLETED*
   - Persist `MempoolEntry.timestamp_ticks` (schema v4) and rebuild the heap on `Blockchain::open`.
   - Run [`purge_expired`](src/lib.rs#L1597-L1666) during startup ([src/lib.rs](src/lib.rs#L918-L935)), dropping stale or missing‑account entries and logging `expired_drop_total`.
   - Update `CONSENSUS.md` with encoding details and migration guidance.
2. **B‑4 Self‑Evict Deadlock Test** — *COMPLETED*
   - Add a panic‑inject harness that forces eviction mid‑admission to prove lock ordering and automatic rollback.
   - Ensure `LOCK_POISON_TOTAL` and `TX_REJECTED_TOTAL{reason=lock_poison}` advance together.
3. **B‑5 Startup TTL Purge** — *COMPLETED*
   - `Blockchain::open` batches mempool rebuilds, invokes [`purge_expired`](src/lib.rs#L1597-L1666) on startup ([src/lib.rs](src/lib.rs#L918-L935)) and restart tests assert `ttl_drop_total` and `startup_ttl_drop_total` advance.
   - `CONSENSUS.md` documents the startup purge, batch size, and telemetry defaults.
4. **Deterministic Eviction & Replay Tests**
   - Unit‑test the priority comparator `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)` for stable ordering.
   - Replay tests cover TTL expiry across restart (`ttl_expired_purged_on_restart`) and `test_schema_upgrade_compatibility` validates v1/v2/v3 → v4 migration with `timestamp_ticks` hydration.
5. **Telemetry & Logging Expansion**
   - Add counters `TTL_DROP_TOTAL`, `ORPHAN_SWEEP_TOTAL`, `LOCK_POISON_TOTAL`,
     `INVALID_SELECTOR_REJECT_TOTAL`, `BALANCE_OVERFLOW_REJECT_TOTAL`,
     `DROP_NOT_FOUND_TOTAL`, and a global
     `TX_REJECTED_TOTAL{reason=*}`.
   - Instrument spans `mempool_mutex`, `eviction_sweep`, and `startup_rebuild`
     capturing sender, nonce, fee_per_byte, and current mempool size.
   - Publish a `serve_metrics` curl example and span list in
     `docs/detailed_updates.md`; keep `rejection_reasons.rs` exercising the
     labelled counters.
6. **Test & Fuzz Matrix**
   - Property tests injecting panics at each admission step to guarantee reservation rollback.
   - 32‑thread fuzz harness with random nonces/fees ≥10 k iterations validating cap, uniqueness, and eviction order.
   - Heap orphan stress test: exceed threshold, trigger rebuild, assert ordering and metric increments.

7. **Admission Atomicity & Ledger Invariants**
   - Use `DashMap::entry` or per-sender mutex to ensure `(sender, nonce)` insert
     and pending-balance reservation form a single atomic operation.
   - Property tests prove pending balances and nonce sets return to prior values on rollback.

8. **Persistence Abstraction**
   - Introduce a storage trait so `SimpleDb` can be swapped for sled/RocksDB
     without touching consensus code.

9. **P2P Skeleton & CLI**
   - Draft a `network` module with basic block/tx gossip and a lightweight
     command-line interface for balance queries, transaction submission, mining,
     and metrics.
7. **Documentation Synchronization**
   - Revise `AGENTS.md`, `Agents-Sup.md`, `Agent-Next-Instructions.md`,
     `AUDIT_NOTES.md`, `CHANGELOG.md`, `API_CHANGELOG.md`, and
     `docs/detailed_updates.md` to reflect every change above, and ensure
     `scripts/check_anchors.py --md-anchors` passes.

## 3. Mid‑Term Milestones
Once the mempool and persistence layers satisfy the above directives, pursue features that build upon this foundation and depend on its determinism.

1. **Durable Storage Backend** – replace `SimpleDb` with a crash‑safe key‑value store. Timestamp persistence from B‑3 enables deterministic rebuilds.
2. **P2P Networking & Sync** – design gossip and fork resolution protocols. A race‑free mempool and replay‑safe persistence are prerequisites.
3. **Node API & Tooling** – expose CLI/RPC once telemetry counters and spans offer operational visibility for remote control.
4. **Dynamic Difficulty Retargeting — COMPLETED** – moving‑average difficulty with bounded step is in place; headers carry `difficulty` and validation enforces the value.
5. **Enhanced Validation & Security** – extend panic‑inject and fuzz coverage to network inputs, enforcing signature, nonce, and fee invariants across peers.
6. **Testing & Visualization Tools** – multi‑node integration tests and dashboards leveraging the telemetry emitted above.

## 4. Long‑Term Vision
Once networking is stable, the project aims to become a modular research platform for advanced consensus and resource sharing.

* **Quantum‑Ready Cryptography** – keep signature and hash algorithms pluggable so post‑quantum schemes can be tested without hard forks.
* **Proof‑of‑Resource Extensions** – reward storage, bandwidth and compute contributions in addition to PoW.
* **Layered Ledger Architecture** – spawn child chains or micro‑shards for heavy compute and data workloads, all anchoring back to the base chain.
* **On‑Chain Governance** – continuous proposal and voting mechanism to upgrade protocol modules in a permissionless fashion.

## 5. Key Principles for Contributors

* **Every commit must pass** `cargo fmt`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --all --release`, and `pytest`.
* `scripts/run_all_tests.sh` auto-detects optional features via `cargo metadata | jq`;
  if `jq` is missing, it warns and proceeds without those features.
* Failing `clippy` does not change runtime behaviour; it flags style,
  documentation, or potential bug risks.
* **No code without spec** – if the behavior is not described in `AGENTS.md` or this supplement, document it first.
* **Explain your reasoning** in PR summaries. Future agents must be able to trace design decisions from docs → commit → code.
* **Operational Rigor** – this repository does not create real tokens or investment opportunities, yet every change assumes eventual main-net exposure.

---

### Disclaimer
The information herein is provided without warranty and does not constitute investment advice. Use the software at your own risk and consult the license terms for permitted usage.

---

### AUDIT_NOTES.md (verbatim)

# Agent/Codex Branch Audit Notes

## Recent Fixes
- Enforced compile-time genesis hash verification and centralized genesis hash computation. **COMPLETED/DONE** [commit: e10b9cb]
- Patched `bootstrap.sh` to install missing build tools and hard-fail on venv mismatches. **COMPLETED/DONE** [commit: e10b9cb]
- Isolated chain state into per-test temp directories and cleaned them on drop;
  replay attack prevention test now asserts duplicate `(sender, nonce)` pairs are
  rejected. **COMPLETED/DONE**
- Added mempool priority comparator unit test proving `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)` ordering. **COMPLETED/DONE**
- Introduced TTL-expiry regression test and telemetry counter `ttl_drop_total`; lock-poison drops now advance `lock_poison_total`.
- Unified mempool critical section (`mempool_mutex → sender_mutex`) covering counter
  updates, heap operations, and pending reservations. Concurrency test
  `flood_mempool_never_over_cap` proves the size cap.
- Orphan sweeps rebuild the heap when `orphan_counter > mempool_size / 2`,
  emit `ORPHAN_SWEEP_TOTAL`, and reset the counter.
- `maybe_spawn_purge_loop` reads `TB_PURGE_LOOP_SECS`/`--mempool-purge-interval`
  and periodically calls `purge_expired`, advancing TTL and orphan-sweep metrics
  even without new transactions.
- Serialized `timestamp_ticks`, rebuilt the mempool on startup, and invoked
  `purge_expired` to drop expired or missing-account entries while logging
  `expired_drop_total` and advancing `ttl_drop_total`.
- **B‑5 Startup TTL Purge — COMPLETED** – `Blockchain::open` batches mempool entries,
  invokes [`purge_expired`](src/lib.rs#L1597-L1666) on startup
  ([src/lib.rs](src/lib.rs#L918-L935)), records `expired_drop_total`, and
  advances `ttl_drop_total` and `startup_ttl_drop_total`.
- Panic-inject eviction test proves rollback and advances lock-poison metrics.
- Completed telemetry coverage: counters `ttl_drop_total`, `orphan_sweep_total`,
  `lock_poison_total`, `invalid_selector_reject_total`,
  `balance_overflow_reject_total`, `drop_not_found_total`, and
  `tx_rejected_total{reason=*}` advance on every rejection; spans
  `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
  `startup_rebuild` record sender, nonce, fee-per-byte, and current
  mempool size ([src/lib.rs](src/lib.rs#L1067-L1082),
  [src/lib.rs](src/lib.rs#L1536-L1542),
  [src/lib.rs](src/lib.rs#L1622-L1657),
  [src/lib.rs](src/lib.rs#L879-L889)). `serve_metrics` scrape example
  documented; `rejection_reasons.rs` asserts the labelled counters and
  `admit_and_mine_never_over_cap` confirms capacity during mining.
- Startup rebuild now processes mempool entries in batches and records
  `startup_ttl_drop_total` (expired mempool entries dropped during startup);
  bench `startup_rebuild` compares batched vs
  naive loops.
- Cached serialized transaction sizes inside `MempoolEntry` so
  `purge_expired` computes fee-per-byte without reserializing;
  `scripts/check_anchors.py --md-anchors` now validates Markdown section
  and Rust line links in CI.
- Introduced Pythonic `PurgeLoop` context manager wrapping `ShutdownFlag`
  and `PurgeLoopHandle`; `demo.py` and docs showcase `with PurgeLoop(bc):`.
- `TTL_DROP_TOTAL` and `ORPHAN_SWEEP_TOTAL` counters saturate at
  `u64::MAX`; tests prove `ShutdownFlag.trigger()` stops the purge loop
  before overflow.
- Added direct `spawn_purge_loop` Python binding, enabling manual
  interval selection, concurrent loops, double trigger/join tests, and
  panic injection via `panic_next_purge`.
- Expanded `scripts/check_anchors.py` to crawl `src`, `tests`, `benches`, and
  `xtask` directories with cached file reads and parallel scanning; updated
  tests cover anchors into `tests/` and `run_all_tests.sh` now warns when
  `jq` or `cargo fuzz` is unavailable, skipping feature detection rather than
  aborting.
- `TxAdmissionError` is `#[repr(u16)]` with stable `ERR_*` constants; Python
  exposes `.code` and telemetry `log_event` entries now carry a numeric
  `code` field alongside `reason`.
- Property-based `fee_recompute_prop` test randomizes blocks, coinbases, and
  fees to ensure migrations recompute emission totals and `fee_checksum`
  correctly; `test_schema_upgrade_compatibility` asserts coinbase sums and
  per-block fee hashes for legacy fixtures.
- Archived `artifacts/fuzz.log` and `artifacts/migration.log` with accompanying
  `RISK_MEMO.md` capturing residual risk and review requirements.
- Introduced a minimal TCP gossip layer (`src/net`) with peer discovery and
  longest-chain adoption; `tests/net_gossip.rs` spins up three nodes to
  confirm chain-height convergence.
- Added command-line `node` binary with JSON-RPC for balance queries,
  transaction submission, mining control, and metrics export; flags
  `--mempool-purge-interval` and `--metrics-addr` wire into purge loop and
  Prometheus exporter.
- RPC server migrated to `tokio` with async tasks replacing per-connection threads for scalable handling.
- `tests/node_rpc.rs` now performs a JSON-RPC smoke test, hitting the metrics,
  balance, and mining-control endpoints.
- `tests/test_purge_loop_env.py` now inserts both a TTL-expired transaction
  and an orphaned one by deleting its sender, then verifies
  `ttl_drop_total`, `orphan_sweep_total`, and `mempool_size` counters.
- `Blockchain::open`, `mine_block`, and `import_chain` refresh the public
  `difficulty` field using `expected_difficulty`; `tests/difficulty.rs`
  asserts retargeting doubles or halves difficulty for fast/slow blocks.
- Table-driven `test_tx_error_codes.py` covers all `TxAdmissionError` variants
  (including lock-poison) and asserts each exception's `.code` matches its
  `ERR_*` constant; `tests/logging.rs` parses telemetry JSON and confirms
  accepted and duplicate transactions carry numeric `code` fields.
- `tests/demo.rs` spawns the Python demo with a 10-second timeout, sets
  `TB_PURGE_LOOP_SECS=1`, forces unbuffered output, and sets
  `TB_DEMO_MANUAL_PURGE` to the empty string so the manual path stays
  disabled; demo logs print and persist on failure.
- Added `tests/test_spawn_purge_loop.py` concurrency coverage spawning two
  manual loops with different intervals and cross-order joins to prove clean
  shutdown and idempotent handle reuse.
- `mempool_order_invariant` now checks transaction order equality instead of
  block hash to avoid timestamp-driven divergence.
- README documents the `TB_DEMO_MANUAL_PURGE` flag for the manual
  purge-loop demonstration, and `CONSENSUS.md` records the timestamp-based
  difficulty retargeting window (120 blocks, 1 000 ms spacing, clamp
  ¼–×4).

## Outstanding Blockers
- **Replay & Migration Tests**: restart suite now covers TTL expiry, and `test_schema_upgrade_compatibility` verifies v1/v2/v3 → v4 migration.

The following notes catalogue gaps, risks, and corrective directives observed across the current branch. Each item is scoped to the current repository snapshot. Sections correspond to the original milestone specifications. Where applicable, cited line numbers reference this repository at HEAD.

Note: `cargo +nightly clippy --all-targets -- -D warnings` reports style and
documentation issues. Failing it does not change runtime behaviour but leaves
technical debt.

## 1. Nonce Handling and Pending Balance Tracking
- **Sequential Nonce Enforcement**: `submit_transaction` checks `tx.payload.nonce != sender.nonce + sender.pending_nonce + 1` (src/lib.rs, L427‑L428). This enforces strict sequencing but does not guard against race conditions between concurrent submissions. A thread‑safe mempool should lock the account entry during admission to avoid double reservation.
- **Pending Balance Reservation**: Pending fields (`pending.consumer`, `pending.industrial`, `pending.nonce`) increment on admission and decrement only when a block is mined (src/lib.rs, L454‑L456 & L569‑L575). There is no path to release reservations if a transaction is dropped or replaced; a mempool eviction routine must unwind the reservation atomically. **COMPLETED/DONE** [commit: e10b9cb]
  - Added `drop_transaction` API that removes a mempool entry, restores balances, and clears the `(sender, nonce)` lock.
- **Atomicity Guarantees**: The current implementation manipulates multiple pending fields sequentially. A failure mid‑update (e.g., panic between consumer and industrial adjustments) can leave the account in an inconsistent state. Introduce a single struct update or transactional storage operation to guarantee atomicity.
- **Mempool Admission Race**: Because `mempool_set` is queried before account mutation, two identical transactions arriving concurrently could both pass the `contains` check before the first insert. Convert to a `HashSet` guarded by a `Mutex` or switch to `dashmap` with atomic insertion semantics.
- **Sender Lookup Failure**: `submit_transaction` returns “Sender not found” if account is absent, but there is no API surface to create accounts implicitly. Decide whether zero‑balance accounts should be auto‑created or require explicit provisioning; document accordingly.

## 2. Fee Routing and Overflow Safeguards
- **Selector Bounds**: Fee selector is not range‑checked; out‑of‑range values fall through to undefined behaviour. Check `fee_selector <= 2` and return `ErrInvalidSelector` consistently. **COMPLETED/DONE**
- **Overflow Prevention**: Fee addition uses `checked_add` inconsistently; use saturating add with explicit supply bounds. **COMPLETED/DONE**
  - `apply_fee` clamps per‑tx fees at `MAX_FEE` and checks miner credit against `MAX_SUPPLY`.
  - `submit_transaction` now enforces selector bounds and documents `MAX_FEE` with a CONSENSUS.md reference.
- **Miner Credit Accounting**:
  - Fees are credited directly to the miner inside the per‑transaction loop (src/lib.rs, L602‑L608) instead of being aggregated into `coinbase_consumer/industrial` and applied once. This violates the “single credit point” directive and complicates block replay proofs. **COMPLETED/DONE** [commit: e10b9cb]
  - No `u128` accumulator is used; summing many near‑`MAX_FEE` entries could overflow `u64` before the clamp. Introduce `u128` accumulators for `total_fee_ct` and `total_fee_it`, check against `MAX_SUPPLY`, then cast to `u64` after clamping. **COMPLETED/DONE**

---

### API_CHANGELOG.md (verbatim)

# API Change Log

## Unreleased

### Python
- Python helper `mine_block(txs)` mines a block from signed transactions for quick demos ([src/lib.rs](src/lib.rs)).
- `RawTxPayload` exposes both `from_` and `from` attributes so decoded payloads are accessible via either name ([src/transaction.rs](src/transaction.rs)).
- `TxAdmissionError::LockPoisoned` is returned when a mempool mutex guard is poisoned.
- `TxAdmissionError::PendingLimit` indicates the per-account pending cap was reached.
- `TxAdmissionError::NonceGap` surfaces as `ErrNonceGap` when a nonce skips the expected sequence.
- `TxAdmissionError` instances expose a stable `code` property and constants
  `ERR_*` map each rejection reason to a numeric identifier.
- `decode_payload(bytes)` decodes canonical payload bytes back into `RawTxPayload`.
- `ShutdownFlag` and `PurgeLoopHandle` manage purge threads when used with
  `maybe_spawn_purge_loop`.
- `PurgeLoop(bc)` context manager spawns the purge loop and triggers
  shutdown on exit.
- `maybe_spawn_purge_loop(bc, shutdown)` reads `TB_PURGE_LOOP_SECS` and returns
  a `PurgeLoopHandle` that joins the background TTL cleanup thread.
- `maybe_spawn_purge_loop` now errors when `TB_PURGE_LOOP_SECS` is unset,
  non-numeric, or ≤0; the Python wrapper raises ``ValueError`` with the parse
  message.
- `spawn_purge_loop(bc, interval_secs, shutdown)` spawns the purge loop with a
  manually supplied interval.
- `Blockchain::panic_in_admission_after(step)` panics mid-admission for test harnesses;
  `Blockchain::heal_admission()` clears the flag.
- `Blockchain::panic_next_evict()` triggers a panic during the next eviction and
  `Blockchain::heal_mempool()` clears the poisoned mutex.
- `PurgeLoopHandle.join()` raises `RuntimeError` if the purge thread panicked
  and setting `RUST_BACKTRACE=1` appends a Rust backtrace to the panic message.
- Dropping `PurgeLoopHandle` triggers shutdown automatically if
  `ShutdownFlag.trigger()` was not called.

### Telemetry
- `TTL_DROP_TOTAL` counts transactions purged due to TTL expiry.
- `STARTUP_TTL_DROP_TOTAL` reports expired mempool entries dropped during
  startup rebuild.
- `ORPHAN_SWEEP_TOTAL` tracks heap rebuilds triggered by orphan ratios.
- `LOCK_POISON_TOTAL` records mutex poisoning events.
- `INVALID_SELECTOR_REJECT_TOTAL`, `BALANCE_OVERFLOW_REJECT_TOTAL`, and
  `DROP_NOT_FOUND_TOTAL` expose detailed rejection counts.
- `TX_REJECTED_TOTAL{reason=*}` aggregates all rejection reasons.

### Kernel
- `service_badge` module introduces `ServiceBadgeTracker` and `Blockchain::check_badges()` which evaluates uptime every 600 blocks.
- `serve_metrics(addr)` exposes Prometheus text over a lightweight HTTP listener.
- `maybe_spawn_purge_loop` reads `TB_PURGE_LOOP_SECS` and spawns a background
  thread that periodically calls `purge_expired`, advancing
  `ttl_drop_total` and `orphan_sweep_total`.
- JSON telemetry logs now include a numeric `code` alongside `reason` for
  each admission event.
- Spans `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
  `startup_rebuild` record sender, nonce, fee-per-byte, and mempool size
  ([src/lib.rs](src/lib.rs#L1067-L1082),
  [src/lib.rs](src/lib.rs#L1536-L1542),
  [src/lib.rs](src/lib.rs#L1622-L1657),
  [src/lib.rs](src/lib.rs#L879-L889)).
- Documented `mempool_mutex → sender_mutex` lock order and added
  `admit_and_mine_never_over_cap` regression to prove the mempool size
  invariant.
- **B ‑5 Startup TTL Purge — COMPLETED** – `Blockchain::open` now invokes [`purge_expired`](src/lib.rs#L1597-L1666)
  ([src/lib.rs](src/lib.rs#L918-L935)), recording
  `ttl_drop_total`, `startup_ttl_drop_total`, and `expired_drop_total` on restart.
- Cached serialized transaction sizes in `MempoolEntry` so `purge_expired`
  avoids reserializing transactions (internal optimization).

### Node CLI & RPC
- Introduced `node` binary exposing `--rpc-addr`, `--mempool-purge-interval`,
  and `--metrics-addr` flags.
- JSON-RPC methods `balance`, `submit_tx`, `start_mining`, `stop_mining`, and
  `metrics` enable external control of the blockchain.
- RPC server uses `tokio` for asynchronous connection handling, removing the thread-per-connection model.
