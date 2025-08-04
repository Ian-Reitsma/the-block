# Agent-Next-Instructions.md — 0.01 % Developer Playbook

> **Read carefully. Execute precisely.**  This file hands off the current
> development state and expectations to the next agent.  Every directive
> presumes you have absorbed `AGENTS.md` and the repository specs.

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

---

## Current Status & Completion Estimate

*Completion score: **48 / 100*** — robust single-node kernel missing network
and ops scaffolding.  See
[Project Completion Snapshot](#project-completion-snapshot--48--100) for
detail.

* **Core consensus/state/tx logic** — 90 %: nonces, dual-token fees, fee
  checksum, and aggregate coinbase are in place; atomicity gaps remain.
* **Database/persistence** — 60 %: schema v3 and migration exist; durable
  backend, rollback, and compaction are pending.
* **Testing/validation** — 60 %: strong coverage for consensus path; lacks
  long-range fuzzing and migration property tests.
* **Demo/docs** — 60 %: demo does not narrate selectors or error paths fully;
  docs need cross-links for new schema edges.
* **Networking (P2P/sync/forks)** — 0 %: no gossip layer, handshake, fork
  resolution, or RPC/CLI.
* **Mid-term engineering infra** — 15 %: CI runs fmt/tests only; coverage,
  fuzzing, schema lint, and contributor automation missing.
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

Treat these as blockers.  Address each with atomic commits and exhaustive
coverage.

1. **Mempool concurrency & atomicity**
   - Touch `src/lib.rs::submit_transaction` and `drop_transaction`.
   - Use `DashMap::entry` or a scoped lock so `(sender, nonce)` check+insert is
     atomic; duplicates must panic in tests.
   - Update `pending_consumer`, `pending_industrial`, and `pending_nonce` in
     one struct write or state transaction; no partial reservations.
   - Extend `tests/mempool_determinism.rs` with a concurrent double-submit
     case; ensure state is unchanged on failure.
2. **Transaction admission hardening**
   - Decide policy for unknown senders in `src/lib.rs::submit_transaction`:
     auto-create via `add_account` or reject; update docs.
   - Replace `saturating_sub` checks with explicit comparisons and raise
     distinct errors (`ErrInsufficientConsumer`, etc.).
   - Emit `tracing` logs for accept/drop with reason codes; cover in
     `tests/test_chain.rs::test_submit_transaction_errors`.
3. **Nonce continuity in mined blocks**
   - In `src/lib.rs::mine_block`, sort by `(from_, nonce)` and skip gaps so
     blocks never contain out-of-order nonces.
   - Update validation (`validate_block`) to treat unknown senders as nonce `0`
     and reject duplicates.
   - Add `tests/test_chain.rs::test_mine_block_nonce_gaps`.
4. **Difficulty verification stub**
   - Add `expected_difficulty(height: u64) -> u32` in `src/lib.rs` (or
     `difficulty.rs`) returning the current constant.
   - Call it from `validate_block`; reject blocks where header.difficulty
     mismatches expectation.
   - Cover via `tests/test_chain.rs::test_difficulty_stub` with wrong value.
5. **Database schema migration & testing**
   - Refine migration in `src/lib.rs::open_db` / `src/simple_db.rs` to preserve
     total supply; recompute historical fees or flag as legacy.
   - Default new `pending_*` fields to `0` for old accounts.
   - Add fixtures under `chain_db/fixtures/{v1,v2}` and enable
     `tests/test_chain.rs::test_schema_upgrade_compatibility`.
   - Add `tests/test_chain.rs::test_snapshot_rollback` verifying state hashes
     match after rollback.
6. **Expanded demo and usage examples**
   - In `demo.py`, narrate each step: explain nonces (“check numbers”) and
     pending balances before/after submissions.
   - Demonstrate fee selectors `0`, `1`, `2` and an invalid selector to show
     error handling.
7. **Documentation & spec refresh**
   - Update `CHANGELOG.md`, `CONSENSUS.md`, `ECONOMICS.md`, and
     `spec/fee_v2.schema.json` with examples and algebraic fee proofs.
   - Mirror LICENSE text verbatim in README and cross-link spec anchors in
     `Agents-Sup.md`.

---

## Mid‑Term Roadmap — 2‑6 Months

Plan and prototype these once immediate items are merged:

1. **Persistent storage backend**: replace `SimpleDb` with a durable KV store.
2. **P2P networking & sync**: node identities, handshake, gossip, fork
   resolution, and feature flag exchange.
3. **Node API and tooling**: CLI/RPC interface plus wallet/explorer scripts.
4. **Dynamic difficulty retargeting**: implement moving-average algorithm and
   propagate updated targets.
5. **Enhanced validation & security**: reorder checks, enforce uniqueness, and
   sandbox state changes.
6. **Comprehensive testing & fuzzing**: property tests for economic invariants,
   cross-language consistency, cargo-fuzz harnesses, and multi-node
   integration tests.
7. **Continuous integration improvements**: coverage gates ≥95 % on consensus,
   JSON schema lint, clippy/fmt enforcement, contributor guide updates.
8. **Observability & DevOps tooling**: Prometheus metrics, Grafana dashboards,
   risk memos, and an emergency fee kill-switch.
9. **Governance & upgrade path planning**: feature flag templates, protocol
   version handshake, snapshot tools.

---

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

## Project Completion Snapshot — 48 / 100

The kernel is robust but not yet investor-ready.  Score components:

* **Core consensus/state/tx logic** ≈ 90 %: nonces, pending balances, dual-token
  fees, aggregate coinbase, and fee checksum are implemented and tested.
  Atomicity gaps remain but are hardening items.
* **Database/persistence** ≈ 60 %: schema v3, migration, and revert logic exist,
  yet no durable backend, compaction, or crash recovery.
* **Testing/validation** ≈ 60 %: consensus paths are covered, but fuzzing,
  snapshot integrity, and migration property tests are missing.
* **Demo/docs** ≈ 60 %: demo lacks exhaustive fee/nonce narratives; docs need a
  full refresh and cross-links for new schema edges.
* **Networking (P2P/sync/forks)** 0 %: no gossip, fork resolution, handshake, or
  RPC/CLI exists.
* **Mid-term engineering infra** ≈ 15 %: CI enforces fmt/tests but lacks
  coverage, fuzz, JSON schema lint, Grafana, or contributor automation.
* **Upgrade/governance** 0 %: no fork artifacts, snapshot tools, or governance
  docs.
* **Long-term vision** 0 %: quantum safety, resource proofs, sharding, and
  on-chain governance remain conceptual only.

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

**Scores**: Solana 84, Ethereum 68, Bitcoin 50, Pi Network 25, The‑Block 48.

**Structural advantages**

* Spec rigor with cross-language determinism.
* Dual-token economics unique among majors.
* Quantum-ready hooks for future cryptography.
* Rust-first safety with `#![forbid(unsafe_code)]`.
* Canonical fee checksum per block.

**Current deficits**

* No P2P or sync.
* Static difficulty and in-memory DB.
* Absent upgrade governance and tooling.

**Path to 80 +**: deliver networking & sync (+15), dynamic difficulty (+5),
persistent storage (+5), CLI/RPC/explorer (+3), governance artifacts (+4),
testnet burn-in & audits (+5), ecosystem tooling (+5).

| Chain | Score | Strengths                | Liabilities                   |
|-------|-------|--------------------------|-------------------------------|
| Solana | 84 | high TPS, sub-sec finality | hardware-heavy, outages      |
| Ethereum | 68 | deep ecosystem, rollups   | 15 TPS base, high fees       |
| Bitcoin | 50 | longest uptime, strong PoW | 10‑min blocks, limited script|
| Pi Network | 25 | large funnel, mobile UX   | opaque consensus, closed code|
| The‑Block | 48 | spec-first, dual-token, Rust | no P2P, static diff, mem DB |

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
4. Run `cargo fmt`, `cargo clippy --all-targets --all-features`, and
   `cargo test --all` before committing.
5. Update docs and specs alongside code.  Every new invariant needs a proof or
   reference in `Agents-Sup.md` or the appropriate spec.
6. Open a PR referencing this file in the summary, detailing tests and docs.
7. Include file and command citations in the PR per `AGENTS.md` §9.

Stay relentless.  Mediocrity is a bug.

