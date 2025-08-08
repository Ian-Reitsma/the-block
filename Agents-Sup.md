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

1. **B‑1 Over‑Cap Race — Global Mempool Mutex**
   - Guard `submit_transaction`, `drop_transaction`, and `mine_block` with `mempool_mutex → sender_mutex`.
   - Enclose counter updates, heap operations, and pending balance/nonce reservations in the critical section.
   - Regression tests must prove `max_mempool_size` is never exceeded under concurrent submission or mining.
2. **B‑2 Orphan Sweep Policy**
   - Maintain `orphan_counter`; rebuild the heap when `orphan_counter > mempool_size / 2`.
   - TTL purge and drop paths decrement the counter; emit `ORPHAN_SWEEP_TOTAL`.
   - Document ratio and sweep behaviour in `CONSENSUS.md` and this supplement.
3. **B‑3 Timestamp Persistence**
   - Persist `MempoolEntry.timestamp_ticks` (schema v4) and rebuild the heap on `Blockchain::open`.
   - Run `purge_expired` during startup, dropping stale or missing‑account entries and logging `expired_drop_total`.
   - Update `CONSENSUS.md` with encoding details and migration guidance.
4. **B‑4 Self‑Evict Deadlock Test**
   - Add a panic‑inject harness that forces eviction mid‑admission to prove lock ordering and automatic rollback.
   - Ensure `LOCK_POISON_TOTAL` and `TX_REJECTED_TOTAL{reason=lock_poison}` advance together.
5. **Deterministic Eviction & Replay Tests**
   - Unit‑test the priority comparator `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)` for stable ordering.
   - Extend replay tests to cover TTL expiry across restart and re‑enable `test_schema_upgrade_compatibility` for v3→v4.
6. **Telemetry & Logging Expansion**
   - Add counters `TTL_DROP_TOTAL`, `ORPHAN_SWEEP_TOTAL`, `LOCK_POISON_TOTAL` and a global `TX_REJECTED_TOTAL{reason=*}`.
   - Instrument spans `mempool_mutex`, `eviction_sweep`, and `startup_rebuild` capturing sender, nonce, fee_per_byte, and current mempool size.
   - Publish a `serve_metrics` curl example and span list in `docs/detailed_updates.md`.
7. **Test & Fuzz Matrix**
   - Property tests injecting panics at each admission step to guarantee reservation rollback.
   - 32‑thread fuzz harness with random nonces/fees ≥10 k iterations validating cap, uniqueness, and eviction order.
   - Heap orphan stress test: exceed threshold, trigger rebuild, assert ordering and metric increments.
8. **Documentation Synchronization**
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

