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

> **Tip:** After any `.rs` or `Cargo.toml` change, run `maturin develop --release --features telemetry` to rebuild and re‑install the Python module in‑place.

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

1. **Lint** — `cargo fmt -- --check` + `black --check` + `ruff check .` + `python scripts/check_anchors.py --md-anchors`.
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

### Test & Fuzz Matrix
- Property test: inject panics at each admission step to verify reservation rollback and heap invariants.
- 32‑thread fuzz harness: random fees and nonces for ≥10 k iterations asserting capacity and per-account uniqueness.
- Heap orphan stress test: exceed the orphan threshold, trigger rebuild, and assert ordering and metrics.

- Mirror these directives in `Agents-Sup.md`, `Agent-Next-Instructions.md`,
  and `AUDIT_NOTES.md`.
- Keep `CHANGELOG.md` and `API_CHANGELOG.md` synchronized with new
  errors, metrics, and flags.
- `scripts/check_anchors.py --md-anchors` validates Markdown headings and
  Rust line anchors; CI rejects any broken link.

---

See [README.md#disclaimer](README.md#disclaimer) for project disclaimer and licensing terms.

**Remember:** *Every line of code must be explainable by a corresponding line in this document or the linked specs.* If not, write the spec first.
