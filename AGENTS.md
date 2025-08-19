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

### 17.3 Operating Mindset

- 0.01% standard: spec citations, `cargo test --all`, zero warnings.
- Atomicity and determinism: no partial writes, no nondeterminism.
- Spec‑first: patch specs before code when unclear.
- Logging and observability: instrument changes; silent failures are bugs.
- Security assumptions: treat inputs as adversarial; validations must be total and explicit.
- Granular commits: single logical changes; every commit builds, tests, and lints cleanly.

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
    `cargo metadata | jq`, warns when `jq` or `cargo fuzz` is absent, and
    now auto-activates the repo's `.venv` when present so contributors can
    invoke it directly. The telemetry regression test `startup_ttl_purge_increments_metrics`
    is `#[serial]` to avoid counter races during parallel test runs.
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

## Commit & PR Protocol <a id="9--commit--pr-protocol"></a>

1. Commit single, atomic changes; each commit must build, lint, and test cleanly.
2. Reference relevant specs in commit messages when altering consensus or APIs.
3. In PR descriptions, include file and command citations demonstrating tests and lints.
4. Link to `AGENTS.md` in the PR summary so reviewers can trace requirements.

## Handoff Checklist for the Next Agent <a id="174-handoff-checklist"></a>

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

## 20 · Audit & Risk Notes (verbatim) <a id="audit-appendix"></a>

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
## 19. Commit History Review Highlights
- The last commit removed leftover badge artifacts, but a systematic audit should validate no SVG or badge workflows remain in submodules or documentation.
- Merge commits (`Fee Routing v2` and `Full-Lifecycle Hardening`) group broad changes; future work should split features into smaller, auditable commits to simplify bisecting and review.
- Early history contains large binary blobs (`pixi.lock` with thousands of lines). A repository rewrite to purge these from git history would reduce clone time and improve auditability.

## 20. Repository Hygiene
- `analysis.txt` and other scratch files live at repo root; convert such documents into tracked design notes under `docs/` or remove them to avoid confusion.
- Ensure every script in `scripts/` has `set -euo pipefail` and consistent shebangs; `scripts/run_all_tests.sh` guards against missing `jq` and `cargo-fuzz`, warning and continuing instead of aborting, and auto-activates `.venv` when available so it can be run without manual sourcing.
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
