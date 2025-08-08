# AGENTS.md — **The‑Block** Top 0.01 % Developer Handbook

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

---

## 1 · Project Mission & Scope

**The‑Block** is a *formally‑specified*, **Rust‑first**, dual‑token, proof‑of‑work/proof‑of‑service blockchain kernel with first‑class Python bindings via **PyO3**.  The repo tracks *only* core consensus, serialization, cryptography, and minimal CLI/wallet tooling—**no web UI, no smart‑contract DSL, no explorer**.  Anything outside that boundary belongs in a sibling repo.

**Design pillars**

* **Determinism ⇢ Reproducibility**: every byte on every node must match for a given height.  All hashes, signatures, and encodings are tested cross‑language (Rust ↔ Python) on CI.
* **Safety ⇢ Rust first**: `#![forbid(unsafe_code)]` is checked in CI; FFI surfaces are minimal and audited.
* **Portability ⇢ x86\_64 & aarch64, Linux/macOS/Windows(WSL)**.
* **Developer Ergonomics ⇢ 0.01 % level**: instant bootstrap; single‑command dev loop; doc‑comment examples compile under `cargo test`.

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

All developers must install the repo's `githooks/pre-commit` hook to ensure the virtualenv is active before committing:

```bash
ln -sf ../../githooks/pre-commit .git/hooks/pre-commit
```

> **Tip:** After any `.rs` or `Cargo.toml` change, run `maturin develop --release` to rebuild and re‑install the Python module in‑place.

---

## 5 · Build & Install Matrix

| Scenario               | Command                                               | Output                               |
| ---------------------- | ----------------------------------------------------- | ------------------------------------ |
| Rust‑only dev loop     | `cargo test --all`                                    | runs lib + test binaries             |
| PyO3 wheel (manylinux) | `maturin build --release --features extension-module` | `target/wheels/the_block‑*.whl`      |
| In‑place dev install   | `maturin develop --release`                           | `import the_block` works in `.venv`  |
| Audit + Clippy         | `cargo clippy --all-targets -- -D warnings`           | zero warnings allowed                |
| Benchmarks             | `cargo bench`                                         | Criterion HTML in `target/criterion` |

> Clippy checks style and potential bugs; failing pedantic lints does not
> affect runtime behaviour but leaves technical debt.
>
> **CI will fail** any PR that leaves `clippy` warnings, `rustfmt` diffs, or
> test failures.

---

## 6 · Testing Strategy

1. **Unit Tests** (`#[cfg(test)]` in each module) — fast, no I/O.
2. **Property Tests** (proptest) — randomized blockchain invariants (`tests/test_chain.rs`).
3. **Cross‑Language Determinism** — Python ↔ Rust serialization byte‑for‑byte equality for 100 random payloads (`tests/test_determinism.py`).
4. **Fuzzing** (`cargo fuzz run verify_sig`) — signature verification stability, 10 k iterations on CI.
5. **Benchmarks** (Criterion) — `verify_signature` must stay < 50 µs median on Apple M2.

Run all locally via:

```bash
./scripts/run_all_tests.sh   # wrapper calls cargo, pytest, fuzz (quick), benches (optional)
```

---

## 7 · Continuous Integration

CI is GitHub Actions; each push/PR runs **seven** jobs:

1. **Lint** — `cargo fmt -- --check` + `black --check` + `ruff check .`.
2. **Build Matrix** — Linux/macOS/Windows in debug & release.
3. **Tests** — `cargo test --all --release` + `pytest`.
4. **Cargo Audit** — `cargo audit -q` must report zero vulnerabilities.
5. **Udeps** — `cargo +nightly udeps --all-targets` ensures no unused dependencies.
6. **Fuzz Smoke** — 1 k iterations per target to catch obvious regressions.
7. **Wheel Build** — `maturin build` and `auditwheel show` to confirm manylinux compliance.
8. **Isolation** — each test uses `unique_path` so every `Blockchain` instance
   writes to a fresh temp directory and removes it on drop.
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

### 10.3 Consensus & Mining

* **PoW**: BLAKE3‑based, adjustable `difficulty_target`, 1‑second block aim.
* **Dual‑Token** emission: consumer vs industrial coinbase split enforced via `block.coinbase_consumer` and `block.coinbase_industrial`. All amount fields use the `TokenAmount` newtype to prevent accidental raw arithmetic.
* **Block validation** order: header → PoW → tx roots → each tx (sig → stateless → stateful).
* **Genesis hash** is computed at build time from the canonical block encoding and checked at compile time.
* **Mempool** uses a `DashMap` plus a binary heap for `O(log n)`
  eviction. All mutations acquire a global `mempool_mutex` followed by a
  per-sender lock. Counter updates, heap pushes/pops, and pending
  balance/nonces execute inside this critical section, preserving the
  invariant `mempool_size ≤ max_mempool_size`. Transactions must pay at
  least the `fee_per_byte` floor and are prioritized by `fee_per_byte`
  (DESC), then `expires_at` (ASC), then transaction hash (ASC). Example
  comparator ordering:

  | fee_per_byte | expires_at | tx_hash | priority |
  |-------------:|-----------:|--------:|---------:|
  |        2000  |          9 | 0x01…   | 1        |
  |        1000  |          8 | 0x02…   | 2        |
  |        1000  |          9 | 0x01…   | 3        |

  The mempool enforces an atomic size cap (default
  1024); once full, new submissions evict the lowest priority entry.
  Orphaned or expired transactions are purged on each submission and
  block import with balances unreserved. Entry timestamps persist across
  restarts and TTLs are enforced on startup, logging `expired_drop_total`.

  Admission surfaces distinct error codes:

  | Code                  | Meaning                                   |
  |-----------------------|-------------------------------------------|
  | `ErrUnknownSender`    | sender not provisioned                    |
  | `ErrInsufficientBalance` | insufficient funds                     |
  | `ErrBadNonce`         | nonce mismatch                            |
  | `ErrInvalidSelector`  | fee selector out of range                 |
  | `ErrBadSignature`     | Ed25519 signature invalid                 |
  | `ErrDuplicateTx`      | `(sender, nonce)` already present         |
  | `ErrTxNotFound`       | transaction missing                       |
  | `ErrBalanceOverflow`  | balance addition overflow                 |
  | `ErrFeeOverflow`      | fee ≥ 2^63                                |
  | `ErrFeeTooLow`        | below `min_fee_per_byte`                  |
  | `ErrMempoolFull`      | capacity exceeded                         |
  | `ErrPendingLimit`     | per-account pending limit hit             |
  | `ErrLockPoisoned`     | mutex guard poisoned                      |

Flags: `--mempool-max`/`TB_MEMPOOL_MAX`, `--mempool-account-cap`/`TB_MEMPOOL_ACCOUNT_CAP`,
`--mempool-ttl`/`TB_MEMPOOL_TTL_SECS`, `--min-fee-per-byte`/`TB_MIN_FEE_PER_BYTE`.

Telemetry metrics: `mempool_size`, `evictions_total`,
`fee_floor_reject_total`, `dup_tx_reject_total`, `ttl_drop_total`,
`lock_poison_total`, `orphan_sweep_total`,
`tx_rejected_total{reason=*}`. Telemetry spans:
`mempool_mutex` (sender, nonce, fpb, mempool_size),
`admission_lock` (sender, nonce),
`eviction_sweep` (mempool_size, orphan_counter),
`startup_rebuild` (expired_drop_total).
`serve_metrics(addr)` starts a minimal HTTP exporter returning
`gather_metrics()` output; e.g. `curl -s localhost:9000/metrics |
grep -E 'orphan_sweep_total|tx_rejected_total'`. Orphan sweeps trigger when
`orphan_counter > mempool_size / 2` and reset the counter. See `API_CHANGELOG.md` for
Python error and telemetry endpoint history. Regression test
`flood_mempool_never_over_cap` floods submissions across threads to assert
the size cap. Panic-inject tests cover admission rollback and
self-eviction to prove recovery. A 32-thread fuzz harness submits random
nonces and fees over 10k iterations to stress capacity and pending nonce
uniqueness.

---

## 11 · Security & Cryptography

| Threat                 | Mitigation                                                |
| ---------------------- | --------------------------------------------------------- |
| Replay across networks | Domain tag with `chain_id` embedded in sign bytes         |
| Serialization mismatch | Cross‑lang determinism test; CI blocks PR if bytes differ |
| Signature malleability | `verify_strict`; rejects non‑canonical sigs               |
| DB corruption          | per-run temp dirs cleaned on drop prevent leftover state |
| Duplicate (sender, nonce) | HashSet guard rejects repeats; replay test active     |
| Unsafe code            | `#![forbid(unsafe_code)]`; CI gate                        |

All cryptographic code is dependency‑pinned; update via dedicated “crypto‑upgrade” PRs.

---

## 12 · Persistence & State

* **Storage** is an in-memory map; data is ephemeral and written to disk via
  higher-level tooling.
* `Blockchain` stores its backing `path` and its `Drop` impl removes that
  directory. `Blockchain::new(path)` expects a unique temp directory; tests call
  `unique_path()` to avoid state leakage.
* No built-in snapshot/backups yet; persistence layer open for future work.

---

## 13 · Troubleshooting Playbook

| Symptom                            | Likely Cause                         | Fix                                                              |
| ---------------------------------- | ------------------------------------ | ---------------------------------------------------------------- |
| `ModuleNotFoundError: the_block`   | Wheel built but not installed        | `maturin develop --release`                                      |
| `libpython3.12.so` linked in wheel | Forgot `--features extension-module` | Re‑build wheel with flag or make feature default in `Cargo.toml` |
| Same tx hash repeats               | `nonce` missing / fake sig           | Ensure unique nonce & real signature                             |
| `cargo test` fails on CI only      | Missing system pkg                   | Check GitHub matrix log; patch `bootstrap.sh`                    |

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
  `orphan_counter > mempool_size / 2`. Purge and drop paths decrement the
  counter and `ORPHAN_SWEEP_TOTAL` telemetry records each sweep.

### B‑3 · Timestamp Persistence — **COMPLETED**
- Serialize `MempoolEntry.timestamp_ticks` in schema v4 and rebuild the heap during `Blockchain::open`.
- Drop expired or missing-account entries on startup, logging `expired_drop_total`.
- Update `CONSENSUS.md` with encoding details and migration notes.

### B‑4 · Eviction Deadlock Proof — **COMPLETED**
- Provide a panic‑inject test that forces eviction mid‑admission to demonstrate lock ordering and full rollback.
- Record `LOCK_POISON_TOTAL` and rejection reasons on every failure path.

### B‑5 · Startup TTL Purge
- Ensure `purge_expired()` runs during `Blockchain::open` and is covered by a restart test that proves `ttl_drop_total` advances.
- Spec the startup purge behaviour and default telemetry in `CONSENSUS.md` and docs.

### Deterministic Eviction & Replay Safety
- Unit‑test the priority comparator `(fee_per_byte DESC, expires_at ASC, tx_hash ASC)` and prove ordering stability.
- Replay suite includes `ttl_expired_purged_on_restart` for TTL expiry and `test_schema_upgrade_compatibility` verifying v1/v2/v3 disks migrate to v4, hydrating `timestamp_ticks`.

### Telemetry & Logging
- Add counters `TTL_DROP_TOTAL`, `ORPHAN_SWEEP_TOTAL`, `LOCK_POISON_TOTAL`, `INVALID_SELECTOR_REJECT_TOTAL`, `BALANCE_OVERFLOW_REJECT_TOTAL`, and `DROP_NOT_FOUND_TOTAL` and ensure `TX_REJECTED_TOTAL{reason=*}` advances on every rejection.
- Instrument spans `mempool_mutex`, `eviction_sweep`, and `startup_rebuild` capturing sender, nonce, fee_per_byte, and mempool size.
- Document a `curl` scrape example for `serve_metrics` output in `docs/detailed_updates.md` and keep `rejection_reasons.rs` exercising the labelled counters.

### Test & Fuzz Matrix
- Property test: inject panics at each admission step to verify reservation rollback and heap invariants.
- 32‑thread fuzz harness: random fees and nonces for ≥10 k iterations asserting capacity and per-account uniqueness.
- Heap orphan stress test: exceed the orphan threshold, trigger rebuild, and assert ordering and metrics.

### Documentation
- Mirror these directives in `Agents-Sup.md`, `Agent-Next-Instructions.md`, and `AUDIT_NOTES.md`.
- Keep `CHANGELOG.md` and `API_CHANGELOG.md` synchronized with new errors, metrics, and flags.

---

See [README.md#disclaimer](README.md#disclaimer) for project disclaimer and licensing terms.

**Remember:** *Every line of code must be explainable by a corresponding line in this document or the linked specs.* If not, write the spec first.
