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
4. **Temp DB isolation** for tests; `Blockchain::new(path)` creates per-run
   directories and `test_replay_attack_prevention` enforces `(sender, nonce)`
   dedup.
5. **Telemetry expansion**: HTTP metrics exporter, `ttl_drop_total`,
   `lock_poison_total`, `orphan_sweep_total`,
   `invalid_selector_reject_total`, `balance_overflow_reject_total`,
   `drop_not_found_total`, `tx_rejected_total{reason=*}`, and span coverage
   for `mempool_mutex`, `admission_lock`, `eviction_sweep`, and
   `startup_rebuild` capturing sender, nonce, fee_per_byte, and
   mempool_size ([`src/lib.rs`](src/lib.rs#L1053-L1068),
   [`src/lib.rs`](src/lib.rs#L1522-L1528),
  [`src/lib.rs`](src/lib.rs#L1603-L1637)). Comparator ordering test for
   mempool priority.
6. **Mempool atomicity**: global `mempool_mutex → sender_mutex` critical section with
   counter updates, heap ops, and pending balances inside; orphan sweeps rebuild
   the heap when `orphan_counter > mempool_size / 2` and emit `ORPHAN_SWEEP_TOTAL`.
7. **Timestamp persistence & eviction proof**: mempool entries persist
   `timestamp_ticks` for deterministic startup purge; panic-inject eviction test
   proves lock-poison recovery.

---

## Current Status & Completion Estimate

*Completion score: **56 / 100*** — robust single-node kernel with improved
telemetry yet still lacking network and ops scaffolding.  See
[Project Completion Snapshot](#project-completion-snapshot--56--100) for
detail.

* **Core consensus/state/tx logic** — 92 %: fee routing, nonce tracking,
  comparator ordering, and global atomicity via `mempool_mutex →
  sender_mutex` are in place.
* **Database/persistence** — 65 %: schema v4 persists admission ticks; durable
  backend pending.
* **Testing/validation** — 68 %: admission and eviction panic tests in place;
  long-range fuzzing and migration property tests remain.
* **Demo/docs** — 62 %: docs track new metrics but still lack cross-links and
  startup rebuild details.
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

Treat the following as blockers. Implement each with atomic commits, exhaustive tests, and cross‑referenced documentation.

1. **Deterministic Eviction & Replay Tests**
   - Existing comparator test must remain; extend to stable ordering after heap rebuild.
   - `ttl_expired_purged_on_restart` exercises TTL expiry across restarts.
   - `test_schema_upgrade_compatibility` verifies v1/v2/v3 → v4 migration.
2. **Telemetry & Logging Expansion**
   - Add counters `TTL_DROP_TOTAL`, `ORPHAN_SWEEP_TOTAL`, `LOCK_POISON_TOTAL`,
     `INVALID_SELECTOR_REJECT_TOTAL`, `BALANCE_OVERFLOW_REJECT_TOTAL`,
     `DROP_NOT_FOUND_TOTAL`, plus global `TX_REJECTED_TOTAL{reason=*}` with
     regression tests for each labelled rejection.
   - Instrument spans `mempool_mutex`, `admission_lock`, `eviction_sweep`,
     and `startup_rebuild` capturing sender, nonce, fee_per_byte,
     mempool_size ([`src/lib.rs`](src/lib.rs#L1053-L1068),
     [`src/lib.rs`](src/lib.rs#L1522-L1528),
     [`src/lib.rs`](src/lib.rs#L1603-L1637)).
   - Document scrape example for `serve_metrics` and span list in
     `docs/detailed_updates.md` and specs.
3. **Test & Fuzz Matrix**
   - Property tests injecting panics at each admission step verifying reservation rollback and metrics.
   - 32‑thread fuzz harness with random fees/nonces for ≥10 k iterations exercising cap and uniqueness.
   - Heap orphan stress test exceeding threshold and asserting rebuild metrics.
4. **Documentation Synchronization**
   - Update `AGENTS.md`, `Agents-Sup.md`, `Agent-Next-Instructions.md`, `AUDIT_NOTES.md`, `CHANGELOG.md`, `API_CHANGELOG.md`, and `docs/detailed_updates.md` for all of the above.

---

## Mid‑Term Roadmap — 2‑6 Months
Once the immediate blockers are merged, build outward while maintaining determinism and observability established above.

1. **Durable storage backend** – replace `SimpleDb` with a crash-safe key‑value store. B‑3’s timestamp persistence is prerequisite.
2. **P2P networking & sync** – design gossip and fork-resolution protocols; a race-free mempool and replay-safe persistence prevent divergence.
3. **Node API & tooling** – expose RPC/CLI once telemetry counters and spans enable operational monitoring.
4. **Dynamic difficulty retargeting** – implement moving-average algorithm; depends on reliable timestamps and startup rebuild.
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

## Project Completion Snapshot — 52 / 100

The kernel is progressing but still far from investor-ready. Score components:

* **Core consensus/state/tx logic** ≈ 92 %: fee routing, pending balances, dual-token fees, and comparator ordering are proven; global mempool mutex still pending.
* **Database/persistence** ≈ 60 %: schema v3 migration exists, but timestamp persistence, durable backend, and rollback tooling are absent.
* **Testing/validation** ≈ 62 %: comparator and panic-inject tests exist, yet eviction, replay, and long-range fuzz gaps remain.
* **Demo/docs** ≈ 62 %: metrics and comparator documented; startup rebuild algorithm and API changelog coverage missing.
* **Networking (P2P/sync/forks)** 0 %: no gossip, fork resolution, handshake, or RPC/CLI.
* **Mid-term engineering infra** ≈ 15 %: CI enforces fmt/tests but lacks coverage, fuzz, schema lint, or contributor automation.
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

**Scores**: Solana 84, Ethereum 68, Bitcoin 50, Pi Network 25, The‑Block 52.

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
| The‑Block | 52 | spec-first, dual-token, Rust | no P2P, static diff, mem DB |

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

