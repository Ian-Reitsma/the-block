# the‑block

> **A formally‑specified, dual‑token blockchain kernel written in Rust with first‑class Python bindings.**  Zero unsafe code, deterministic serialization, cross‑platform builds, one‑command bootstrap.
> Built from day one for real-world deployment; every example uses the same APIs shipped to production nodes.

---

## Table of Contents

1. [Why the‑block?](#why-the‑block)
2. [Quick Start](#quick-start)
3. [Installation & Bootstrap](#installation--bootstrap)
4. [Build & Test Matrix](#build--test-matrix)
5. [Using the Python Module](#using-the-python-module)
6. [Architecture Primer](#architecture-primer)
7. [Project Layout](#project-layout)
8. [Contribution Guidelines](#contribution-guidelines)
9. [Security Model](#security-model)
10. [License](#license)

---

## Why the‑block?

* **Dual‑Token Economics** – consumer & industrial coins emitted per block, supporting differentiated incentive layers.
* **Ed25519 + BLAKE3** – modern cryptography with strict verification and domain separation.
* **Rust first** – `#![forbid(unsafe_code)]`, MSRV 1.74, formally verifiable components.
* **PyO3 bindings** – import and use the chain directly from Python for rapid prototyping, data‑science, or wallet scripting.
* **One‑command bootstrap** – `bootstrap.sh`/`bootstrap.ps1` installs every prerequisite (Rust, Python 3.12, maturin, clippy, Node 20) and builds a development wheel.
* **Deterministic state** – cross‑language tests guarantee every node serializes, signs, and hashes identically.
* **Schema versioned DB** – the node refuses to open newer databases without explicit migration, preventing silent corruption.
* **CI‑first** – GitHub Actions matrix across Linux, macOS, and Windows (WSL) ensures builds stay green.
---

## Quick Start

```bash
# Unix/macOS
bash ./bootstrap.sh          # installs toolchains + builds + tests + wheel
python demo.py               # mines a few blocks & prints balances (requires telemetry feature)

# Windows (PowerShell)
./bootstrap.ps1              # run as admin to install VS Build Tools via choco
python demo.py               # same demo
```

> Look for `🎉 demo completed` in the console—if you see it, the kernel, bindings, and demo all worked.

## Disclaimer

This repository is a **research prototype**. Running the demo does **not** create
or transfer any real cryptocurrency. Nothing herein constitutes financial advice
or an invitation to invest. Use the code at your own risk and review the license
terms carefully.

---

## Installation & Bootstrap

| OS                   | Command                     | Notes                                                                                |
| -------------------- | --------------------------- | ------------------------------------------------------------------------------------ |
| **Linux/macOS/WSL2** | `bash ./bootstrap.sh`       | idempotent; safe to rerun; uses apt/dnf/homebrew detection                           |
| **Windows 10/11**    | `./bootstrap.ps1` *(Admin)* | installs Rust, Python 3.12 (via pyenv‑win), VS 2022 Build Tools; reboots if required |

Bootstrap steps:

1. Install or update **Rust** toolchain (`rustup`, nightly optional).
2. Install **Python 3.12** + headers, create `.venv`, and activate.
3. `pip install maturin black pytest` into the venv.
4. `cargo install maturin` (if missing) and build wheel via `maturin develop --release --features telemetry`.
5. Optional: install **Node 20** via `nvm` (for tooling not yet in repo).
6. Run `cargo test --all --release`, `.venv/bin/python -m pytest`, and
   `.venv/bin/python demo.py` to verify the toolchain and bindings.
   The demo asserts metrics only if the module was built with `--features telemetry`.

> Need CUDA, Docker, or GPU?  Not here—this repo is CPU‑only and self‑contained.

---

## Build & Test Matrix

| Task | Command | Expected Output |
| --- | --- | --- |
| Rust unit + property tests | `cargo test --all --release` | All tests green |
| In-place dev install | `maturin develop --release --features telemetry` | Module importable in venv |
| Python tests | `.venv/bin/python -m pytest` | All tests pass |
| End-to-end demo | `.venv/bin/python demo.py` | `✅ demo completed` (requires `--features telemetry`) |
| Lint / Style | `cargo fmt -- --check` | No diffs |

All tests run in isolated temp directories via `unique_path`, preventing state
leakage between cases. Paths are removed once the chain drops.

CI runs all of the above across **Linux‑glibc 2.34, macOS 12, and Windows 11 (WSL 2)**.  A red badge on `main` blocks merges.

---

## Using the Python Module

```python
from the_block import RawTxPayload, sign_tx, verify_signed_tx, mine_block
from nacl.signing import SigningKey  # or use ed25519_dalek in Rust

# 1 Generate keypair
sk = SigningKey.generate()
pk = sk.verify_key

# 2 Create and sign a transaction
payload = RawTxPayload(
    from_ = pk.encode().hex(),
    to    = "deadbeef" * 4,
    amount_consumer   = 1_000,
    amount_industrial = 0,
    fee               = 10,
    nonce             = 0,
    memo              = b"hello‑world",
)

stx = sign_tx(sk.encode(), payload)
assert verify_signed_tx(stx)

# 3 Mine a block (CPU PoW)
block = mine_block([stx])  # returns dict‑like Python object
print(block["header"]["hash"])
```

All functions return Python‑native types (`dict`, `bytes`, `int`) for simplicity.

---

## Architecture Primer

* **Hashing** – BLAKE3‑256 for both block and transaction IDs (32 bytes).
* **Signature** – Ed25519 strict; signing bytes are `DOMAIN_TAG | bincode(payload)`.
* **Consensus** – simple PoW with adjustable `difficulty_target`.  Future milestones add proof‑of‑service weight.
* **Dual‑Token** – each block’s coinbase emits consumer vs industrial supply; max supply = 20 M each. The header records `coinbase_consumer` and `coinbase_industrial` using a `TokenAmount` wrapper so light clients can audit supply without replaying the chain.
* **Storage** – in-memory `SimpleDb` backed by a per-run temp directory.
  `Blockchain::new(path)` removes the directory on drop so state never leaks
  across tests.
* **Fuzzing** – `cargo fuzz run verify_sig` defends against malformed signatures.
* **Extensibility** – modular crates (`crypto`, `blockchain`, `storage`); WASM host planned for smart contracts.

> For a deeper dive, read `docs/signatures.md` and `AGENTS.md`.

---

## Project Layout

```text
src/
  ├── lib.rs           # PyO3 module + re‑exports
  ├── blockchain/      # blocks, headers, mining, validation
  ├── crypto/          # hash, signature, canonical serialization
  └── utils/           # hex helpers, logging, config

bootstrap.sh           # Unix setup script
bootstrap.ps1          # Windows setup script

tests/                 # Rust tests (unit + proptest)
benches/               # Criterion benches
demo.py                # Python end‑to‑end demo
docs/                  # Markdown specs (rendered by mdBook)
docs/detailed_updates.md  # in-depth change log for auditors
API_CHANGELOG.md          # Python errors and telemetry endpoint history
AGENTS.md              # Developer handbook (authoritative)
```

---

## Contribution Guidelines

1. **Fork & branch**: `git checkout -b feat/<topic>`.
2. **Follow coding standards** (Rustfmt, Clippy, Black).
3. **Write tests** for every PR; property tests if possible.
4. **Update docs** (`AGENTS.md`, `docs/`) if behaviour or API changes.
5. **Commit messages** follow Conventional Commits (`feat:`, `fix:`, `refactor:`).
6. **Open PR**; fill template with *Summary*, *Testing*, and *Docs Updated?*.
7. Pull‑request must pass **all CI checks** before merge.

> 🛑  **Never** push directly to `main`.  Squash‑merge only.

---

## Security Model

See [docs/signatures.md](docs/signatures.md) and [AGENTS.md](AGENTS.md#11-security--cryptography) for the full threat matrix.  Highlights:

* **Domain separation** prevents cross‑network replay.
* **Strict signature verification** eliminates malleability.
* **No unsafe Rust** ensures memory safety.
* **Checksummed, deterministic DB** protects state integrity.
* **Fuzz tests** catch parsing edge‑cases before release.

---

## Telemetry & Metrics

Prometheus counters and tracing spans expose mempool health and rejection
reasons for ops tooling.

```bash
TB_TELEMETRY=1 ./target/release/the-block &
curl -s localhost:9000/metrics \
  | grep -E 'mempool_size|startup_ttl_drop_total|invalid_selector_reject_total|tx_rejected_total'
```

Key metrics: `mempool_size`, `evictions_total`, `fee_floor_reject_total`,
`dup_tx_reject_total`, `ttl_drop_total`, `startup_ttl_drop_total` (expired mempool entries dropped during startup),
`lock_poison_total`, `orphan_sweep_total`,
`invalid_selector_reject_total`, `balance_overflow_reject_total`,
`drop_not_found_total`, and `tx_rejected_total{reason=*}`. Spans
[`mempool_mutex`](src/lib.rs#L1066-L1081), [`admission_lock`](src/lib.rs#L1535-L1541),
[`eviction_sweep`](src/lib.rs#L1621-L1656), and [`startup_rebuild`](src/lib.rs#L878-L888) annotate
sender, nonce, fee-per-byte, and sweep details.

Report security issues privately via `security@the-block.dev` (PGP key in `docs/SECURITY.md`).

---

## Disclaimer

This software is an experimental blockchain kernel for research and development. It is not investment advice and comes with no warranty. Use at your own risk.

---

## License

Copyright 2025 IJR Enterprises, Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this project except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
implied. See the [LICENSE](LICENSE) for the specific language
governing permissions and limitations under the License.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this project by you, as defined in the
[LICENSE](LICENSE), shall be licensed as described above, without any
additional terms or conditions.
