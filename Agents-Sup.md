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
* Persistent state lives in a sled database (`chain_db`). `ChainDisk` encapsulates the chain, account map and emission counters. Schema version = 3.

### Python Demo
* `demo.py` creates a fresh chain, mines a genesis block, signs a sample message, submits a transaction and mines additional blocks while printing explanatory output.

### Tests
* Rust property tests under `tests/test_chain.rs` validate invariants (balances never negative, reward decay, duplicate TxID rejection, etc.).
* `tests/test_interop.py` confirms Python and Rust encode transactions identically.

## 2. Immediate Next Steps
The following tasks are ordered from highest urgency to longer‑term milestones. Each new feature must come with unit tests, property tests where appropriate, and documentation updates.

1. **Nonce Enforcement & Pending Ledger**
   - Reject any submitted transaction whose nonce is not exactly `account.nonce + 1`.
   - Track `pending_consumer`, `pending_industrial` and `pending_nonce` to lock funds in the mempool so double spends cannot occur.
2. **Fee Routing & Overflow Clamp**
   - Enforce the fee routing equations documented in `analysis.txt`. Split fees according to `fee_selector` and cap `fee < 2^63` to avoid `div_ceil` overflow.
3. **Difficulty Field Verification**
   - Include `difficulty` in the block header hash and verify that `block.difficulty` equals the network target for the given height.
4. **In‑Block Nonce Continuity**
   - When mining a block, ensure each sender’s transactions appear in strict nonce order with no gaps.
5. **Mempool Deduplication**
   - Maintain a `HashSet<(sender, nonce)>` to reject duplicate payloads and prevent replay spam.
6. **Schema Version Bump and Migration Test**
   - After modifying on‑disk formats, increment `ChainDisk.schema_version` and provide a migration path from older layouts. Add an explicit unit test that loads a v1/v2 database and upgrades it.
7. **Demo Verbosity Enhancements**
   - Expand `demo.py` logging to narrate each phase (keygen, genesis mining, transaction admission, block mining, state update, reward decay). Provide analogies and layman terminology.
8. **Documentation Refresh**
   - Keep `README.md`, `AGENTS.md`, and `docs/detailed_updates.md` synchronized with new behavior. Every consensus change must be explained in prose and referenced from the code.

## 3. Mid‑Term Milestones
After the immediate patches above, focus shifts toward networking and user tooling.

1. **Persistent Storage Refinements** – abstract sled behind a trait so alternative databases or snapshots can be plugged in.
2. **P2P Networking** – design a simple protocol (libp2p recommended) for block and transaction gossip. Implement longest‑chain sync and fork resolution.
3. **CLI / RPC API** – expose node controls via command‑line and/or HTTP so multiple nodes can be orchestrated in tests.
4. **Dynamic Difficulty Retarget** – adjust `difficulty` based on moving average block times to maintain the target interval.
5. **Enhanced Validation** – verify all incoming blocks and transactions from peers: signature checks, PoW target, nonce sequence, and fee accounting.
6. **Testing & Visualization Tools** – provide integration tests that spin up two or more nodes and assert ledger equivalence. Add scripts to pretty‑print chain state for auditors.

## 4. Long‑Term Vision
Once networking is stable, the project aims to become a modular research platform for advanced consensus and resource sharing.

* **Quantum‑Ready Cryptography** – keep signature and hash algorithms pluggable so post‑quantum schemes can be tested without hard forks.
* **Proof‑of‑Resource Extensions** – reward storage, bandwidth and compute contributions in addition to PoW.
* **Layered Ledger Architecture** – spawn child chains or micro‑shards for heavy compute and data workloads, all anchoring back to the base chain.
* **On‑Chain Governance** – continuous proposal and voting mechanism to upgrade protocol modules in a permissionless fashion.

## 5. Key Principles for Contributors

* **Every commit must pass** `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all --release`, and `pytest`.
* **No code without spec** – if the behavior is not described in `AGENTS.md` or this supplement, document it first.
* **Explain your reasoning** in PR summaries. Future agents must be able to trace design decisions from docs → commit → code.
* **Educational Only** – reiterate that this repository does not create real tokens or investment opportunities. The project is a learning platform.

---

### Disclaimer
The information herein is provided for research and educational purposes. The maintainers of **the‑block** do not offer investment advice or guarantee financial returns. Use the software at your own risk and consult the license terms for permitted usage.

