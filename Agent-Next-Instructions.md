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
4. Run `cargo fmt`, `cargo clippy --all-targets --all-features`,
   `cargo test --all`, and `python scripts/check_anchors.py --md-anchors`
   before committing.
5. Update docs and specs alongside code.  Every new invariant needs a proof or
   reference in `Agents-Sup.md` or the appropriate spec.
6. Open a PR referencing this file in the summary, detailing tests and docs.
7. Include file and command citations in the PR per `AGENTS.md` §9.
8. When running `demo.py` (e.g., the `demo_runs_clean` test), set
   `TB_PURGE_LOOP_SECS` to a positive integer such as `1` so the purge
   loop context manager can spawn, force `PYTHONUNBUFFERED=1` for
   real-time logs, and leave `TB_DEMO_MANUAL_PURGE` unset or empty to
   use the context manager; set `TB_DEMO_MANUAL_PURGE=1` to exercise the
   manual shutdown‑flag/handle example instead. The script will invoke
   `maturin develop` automatically if the `the_block` module is missing.
9. For long-running tests (e.g., `reopen_from_snapshot`), set `TB_SNAPSHOT_INTERVAL`
   and lower block counts locally to iterate quickly, then restore canonical
   values before committing.

Stay relentless.  Mediocrity is a bug.

