# Agents Supplement — Strategic Roadmap and Orientation

This document extends `AGENTS.md` with a deep dive into the project's long‑term vision and the immediate development sequence. Read both files in full before contributing.

## 0. Scope Reminder

* **Research Prototype** – The code demonstrates blockchain mechanics for educational review. It is **not** a production network nor financial instrument.
* **Rust First, Python Friendly** – The kernel is implemented in Rust with PyO3 bindings for scripting and tests. Absolutely no unsafe code is allowed.
* **Dual‑Token Ledger** – Balances are tracked in consumer and industrial units. Token arithmetic uses the `TokenAmount` wrapper.

## 1. Current Architecture Overview

### Consensus & Mining
* Proof of Work using BLAKE3 hashes. Difficulty is static today (see `Blockchain.difficulty`).
* Each block stores `coinbase_consumer` and `coinbase_industrial`; the first transaction must match these values.
* Block rewards decay by a factor of `DECAY_NUMERATOR / DECAY_DENOMINATOR` each block.

### Accounts & Transactions
* `Account` maintains balances, nonce and pending totals to prevent overspending.
* `RawTxPayload` → `SignedTransaction` using Ed25519 signatures. The canonical signing bytes are `domain_tag || bincode(payload)`.
* Transactions include a `fee_selector` selector (0=consumer, 1=industrial, 2=split) and must use sequential nonces.

### Storage
* Persistent state lives in an in-memory map (`SimpleDb`). `ChainDisk` encapsulates the
  chain, account map and emission counters. Schema version = 3.
* `Blockchain` tracks its `path` and its `Drop` impl removes the directory.
  `Blockchain::new(path)` expects a unique temp directory; tests use
  `unique_path()` to avoid cross-test leakage.

### Mempool Concurrency
* A global `mempool_mutex` guards all mempool mutations before the per-sender
  lock. Counter updates, heap pushes/pops, and pending balance/nonces are
  executed inside this lock order, ensuring the invariant `mempool_size ≤
  max_mempool_size`.
* Entries referencing missing accounts increment an `orphan_counter`; once the
  counter exceeds half the mempool, a sweep drops all orphans, emits
  `ORPHAN_SWEEP_TOTAL`, and resets the counter.

### Telemetry Metrics & Spans
* Metrics: `mempool_size`, `evictions_total`, `fee_floor_reject_total`,
  `dup_tx_reject_total`, `ttl_drop_total`, `startup_ttl_drop_total`,
  `lock_poison_total`, `orphan_sweep_total`, `invalid_selector_reject_total`,
  `balance_overflow_reject_total`, `drop_not_found_total`,
  `tx_rejected_total{reason=*}`.
* Spans: `mempool_mutex` (sender, nonce, fpb, mempool_size),
  `admission_lock` (sender, nonce), `eviction_sweep` (sender, nonce,
  fpb, mempool_size), `startup_rebuild` (sender, nonce, fpb,
  mempool_size). See [`src/lib.rs`](src/lib.rs#L1065-L1081),
  [`src/lib.rs`](src/lib.rs#L1535-L1541), and
  [`src/lib.rs`](src/lib.rs#L1615-L1650).
* `serve_metrics(addr)` exposes Prometheus text; e.g.
  `curl -s localhost:9000/metrics | grep tx_rejected_total`.

### Schema Migrations & Invariants
* Bump `ChainDisk.schema_version` for any on-disk format change and supply a lossless migration routine with tests.
* Each migration must preserve [`INV-FEE-01`](ECONOMICS.md#inv-fee-01) and [`INV-FEE-02`](ECONOMICS.md#inv-fee-02); update `docs/schema_migrations/` with the new invariants.

### Python Demo
* `demo.py` creates a fresh chain, mines a genesis block, signs a sample message, submits a transaction and mines additional blocks while printing explanatory output.

### Tests
* Rust property tests under `tests/test_chain.rs` validate invariants (balances never
  negative, reward decay, duplicate TxID rejection, etc.).
* Fixtures create isolated directories via `unique_path()` and clean them after
  execution so runs remain hermetic.
* `test_replay_attack_prevention` asserts duplicate `(sender, nonce)` pairs are rejected.
* `tests/test_interop.py` confirms Python and Rust encode transactions identically.

## 2. Immediate Next Steps
The following directives are mandatory before any feature expansion. Deliver each with exhaustive tests, telemetry, and cross‑referenced documentation.

1. **B‑3 Timestamp Persistence** — *COMPLETED*
   - Persist `MempoolEntry.timestamp_ticks` (schema v4) and rebuild the heap on `Blockchain::open`.
   - Run `purge_expired` during startup, dropping stale or missing‑account entries and logging `expired_drop_total`.
   - Update `CONSENSUS.md` with encoding details and migration guidance.
2. **B‑4 Self‑Evict Deadlock Test** — *COMPLETED*
   - Add a panic‑inject harness that forces eviction mid‑admission to prove lock ordering and automatic rollback.
   - Ensure `LOCK_POISON_TOTAL` and `TX_REJECTED_TOTAL{reason=lock_poison}` advance together.
3. **B‑5 Startup TTL Purge** — *COMPLETED*
   - `Blockchain::open` batches mempool rebuilds, invokes `purge_expired` on startup and restart tests assert `ttl_drop_total` and `startup_ttl_drop_total` advance.
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
7. **Documentation Synchronization**
   - Revise `AGENTS.md`, `Agents-Sup.md`, `Agent-Next-Instructions.md`, `AUDIT_NOTES.md`, `CHANGELOG.md`, `API_CHANGELOG.md`, and `docs/detailed_updates.md` to reflect every change above.

## 3. Mid‑Term Milestones
Once the mempool and persistence layers satisfy the above directives, pursue features that build upon this foundation and depend on its determinism.

1. **Durable Storage Backend** – replace `SimpleDb` with a crash‑safe key‑value store. Timestamp persistence from B‑3 enables deterministic rebuilds.
2. **P2P Networking & Sync** – design gossip and fork resolution protocols. A race‑free mempool and replay‑safe persistence are prerequisites.
3. **Node API & Tooling** – expose CLI/RPC once telemetry counters and spans offer operational visibility for remote control.
4. **Dynamic Difficulty Retargeting** – implement moving‑average difficulty; requires reliable timestamping and startup rebuild.
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
* Failing `clippy` does not change runtime behaviour; it flags style,
  documentation, or potential bug risks.
* **No code without spec** – if the behavior is not described in `AGENTS.md` or this supplement, document it first.
* **Explain your reasoning** in PR summaries. Future agents must be able to trace design decisions from docs → commit → code.
* **Educational Only** – reiterate that this repository does not create real tokens or investment opportunities. The project is a learning platform.

---

### Disclaimer
The information herein is provided for research and educational purposes. The maintainers of **the‑block** do not offer investment advice or guarantee financial returns. Use the software at your own risk and consult the license terms for permitted usage.

