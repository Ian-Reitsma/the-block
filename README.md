# the‑block

> **A formally‑specified, dual‑token blockchain kernel written in Rust with first‑class Python bindings.**  Zero unsafe code, deterministic serialization, cross‑platform builds, one‑command bootstrap.
> Built from day one for real-world deployment; every example uses the same APIs shipped to production nodes.

---

## Table of Contents

1. [Why the‑block?](#why-the‑block)
2. [Vision & Current State](#vision--current-state)
3. [Quick Start](#quick-start)
4. [Installation & Bootstrap](#installation--bootstrap)
5. [Build & Test Matrix](#build--test-matrix)
6. [Using the Python Module](#using-the-python-module)
7. [Architecture Primer](#architecture-primer)
8. [Project Layout](#project-layout)
9. [Status & Roadmap](#status--roadmap)
10. [Contribution Guidelines](#contribution-guidelines)
11. [Security Model](#security-model)
12. [License](#license)

---

## Why the‑block?

* **Civic‑grade architecture** – one‑second Layer 1 anchors notarized micro‑shards and ties votes to service via a non‑transferable credit meter.
* **Dual‑Token Economics** – consumer & industrial coins emitted per block, supporting differentiated incentive layers.
* **Ed25519 + BLAKE3** – modern cryptography with strict verification and domain separation.
* **Rust first** – `#![forbid(unsafe_code)]`, MSRV 1.74, formally verifiable components.
* **PyO3 bindings** – import and use the chain directly from Python for rapid prototyping, data‑science, or wallet scripting.
* **One‑command bootstrap** – `bootstrap.sh`/`bootstrap.ps1` installs every prerequisite (Rust, Python 3.12, maturin, clippy, Node 20) and builds a development wheel.
* **Deterministic state** – cross‑language tests guarantee every node serializes, signs, and hashes identically.
* **Schema versioned DB** – the node refuses to open newer databases without explicit migration, preventing silent corruption.
* **CI‑first** – GitHub Actions matrix across Linux, macOS, and Windows (WSL) ensures builds stay green.

## Vision & Current State

The‑block aims to be a civic‑grade network where **service guarantees
citizenship**. A one‑second Layer 1 anchors notarized micro‑shards for
sub‑second AI and data workloads. Economics flow through two tradeable
tokens—**Consumer** and **Industrial**—while a non‑transferable service‑credit
meter offsets writes for honest nodes. Governance ties one badge to one vote
and groups nodes into shard districts for bicameral decisions.

The current kernel already provides:

- one‑second block cadence with dynamic difficulty,
- dual‑token fee routing and decay‑based emission,
- purge‑loop infrastructure with TTL/orphan telemetry,
- a minimal TCP gossip layer and JSON‑RPC control surface, and
- a Python demo showcasing fee selectors and nonce rules.

Upcoming work adds durable storage, authenticated peer discovery,
micro‑shard bundle roots, quantum‑ready crypto, and the full badge‑based
governance stack. See [`agents_vision.md`](agents_vision.md) for the complete
blueprint.
---

## Quick Start

```bash
# Unix/macOS
bash ./bootstrap.sh          # installs toolchains + builds + tests + wheel
python demo.py               # demo with background purge loop (TB_PURGE_LOOP_SECS defaults to 1)

# Windows (PowerShell)
./bootstrap.ps1              # run as admin to install VS Build Tools via choco
python demo.py
```

> Look for `🎉 demo completed` in the console—if you see it, the kernel, bindings, and demo all worked.

Running `demo.py` will attempt to build the `the_block` extension with
`maturin` if it is not already installed. The script installs `maturin` on
the fly when missing, so only a Rust toolchain and build prerequisites are
required.

### Manual purge-loop demonstration

To watch the purge loop being started and stopped explicitly, set
`TB_DEMO_MANUAL_PURGE=1` before invoking the demo:

```bash
TB_DEMO_MANUAL_PURGE=1 python demo.py
```

In this mode `demo.py` calls
`spawn_purge_loop(bc, 1, ShutdownFlag())`, submits a transaction, and then
triggers the flag and joins the handle. Without `TB_DEMO_MANUAL_PURGE` the
demo uses the `PurgeLoop` context manager with `TB_PURGE_LOOP_SECS` (default `1`)
to spawn and cleanly stop the background thread. CI runs the demo with
`TB_PURGE_LOOP_SECS=1`, forces `PYTHONUNBUFFERED=1`, and leaves
`TB_DEMO_MANUAL_PURGE` empty so the context-manager path finishes within the 20‑second budget; the manual
flag/handle variant is covered separately in `tests/test_spawn_purge_loop.py`
using the same 1‑second interval to keep it fast.

## Disclaimer

This repository houses a production‑grade blockchain kernel under active development. Running the demo does **not** create or transfer any real cryptocurrency. Nothing herein constitutes financial advice or an invitation to invest. Use the code at your own risk and review the license terms carefully.

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
| Markdown anchors | `python scripts/check_anchors.py --md-anchors` | No output |

All tests run in isolated temp directories via `tests::util::temp::temp_dir`,
preventing state leakage between cases. These directories are removed
automatically when their handle is dropped.
CI runs all of the above across **Linux‑glibc 2.34, macOS 12, and Windows 11 (WSL 2)**.  A red badge on `main` blocks merges.

### macOS Python framework

`pyo3` looks for the active Python when compiling. On macOS the dynamic
loader may fail with `dyld: Library not loaded: @rpath/libpython*.dylib`
if the build linked against a different interpreter than the one in your
virtual environment. Export `PYO3_PYTHON` and `PYTHONHOME` before building
to select the correct interpreter. The build script adds `$PYTHONHOME/lib`
to the binary's `rpath` so `cargo run` can find `libpython` at runtime:

```bash
export PYO3_PYTHON=$(python3 -c 'import sys; print(sys.executable)')
export PYTHONHOME=$(python3 -c 'import sys, pathlib; print(pathlib.Path(sys.executable).resolve().parents[1])')
cargo build
cargo run --bin node -- --help            # list subcommands
cargo run --bin node -- run --help        # run-specific options
```

Verify that the expected dynamic library exists in the selected Python
home:

```bash
ls "$PYTHONHOME"/lib | grep libpython
```

---

## Node CLI and JSON-RPC

Compile and run a local node with optional metrics export and background TTL
purges:

```bash
cargo run --bin node -- run --rpc-addr 127.0.0.1:3030 \
    --mempool-purge-interval 5 --metrics-addr 127.0.0.1:9100
```

### Wallet key management

The `node` binary doubles as a simple wallet storing keys under
`~/.the_block/keys`:

```bash
# Generate a keypair saved as ~/.the_block/keys/alice.pem
cargo run --bin node -- generate-key alice

# Display the hex address for a key id
cargo run --bin node -- show-address alice

# Sign a transaction JSON payload
cargo run --bin node -- sign-tx alice '{"from_":"<hex>","to":"bob","amount_consumer":1,"amount_industrial":0,"fee":1,"fee_selector":0,"nonce":1,"memo":[]}'
# => <hex-encoded bincode SignedTransaction>

# Import an existing PEM file and show its address
cargo run --bin node -- import-key path/to/key.pem
cargo run --bin node -- show-address key
```
Use the address to query balances or submit transactions over JSON-RPC:

```bash
ADDR=$(cargo run --bin node -- show-address alice)
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"balance","params":{"address":"'"$ADDR"'"}}'
```

### Example RPC session

```bash
# 1. Start a node in one terminal
cargo run --bin node -- run --rpc-addr 127.0.0.1:3030 --mempool-purge-interval 5

# 2. Generate a key and capture its address
cargo run --bin node -- generate-key alice
ADDR=$(cargo run --bin node -- show-address alice)

# 3. Sign and submit a self-transfer
TX=$(cargo run --bin node -- sign-tx alice '{"from_":"'$ADDR'","to":"'$ADDR'","amount_consumer":1,"amount_industrial":0,"fee":1,"fee_selector":0,"nonce":1,"memo":[]}')
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"submit_tx","params":{"tx":"'$TX'"}}'

# 4. Mine a block to include the transaction
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"start_mining","params":{"miner":"'$ADDR'"}}'
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":4,"method":"stop_mining"}'

# 5. Query the updated balance
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":5,"method":"balance","params":{"address":"'$ADDR'"}}'
```

Environment variables influence node behaviour during these sessions:

```
TB_PURGE_LOOP_SECS=1      # (optional) purge loop interval; demo.py defaults to 1
PYTHONUNBUFFERED=1        # unbuffered output for Python demos/tests
TB_DEMO_MANUAL_PURGE=1    # require manual purge-loop shutdown
```

Interact with the node via JSON-RPC; requests use `jsonrpc` and an incrementing `id`:

```bash
# Query balances
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"balance","params":{"address":"alice"}}'
# => {"jsonrpc":"2.0","result":{"consumer":0,"industrial":0},"id":1}

# Submit a hex‑encoded bincode transaction (use an actual hex string)
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"submit_tx","params":{"tx":"<hex>"}}'
# => {"jsonrpc":"2.0","result":{"status":"ok"},"id":2}

# Start and stop mining
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"start_mining","params":{"miner":"miner"}}'
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":4,"method":"stop_mining"}'

# Dump metrics over RPC
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":5,"method":"metrics"}'
```

A minimal Python client for quick experimentation:

```python
import json, socket

def rpc(method, params=None, *, id=1):
    body = json.dumps({"jsonrpc": "2.0", "id": id, "method": method, "params": params or {}})
    with socket.create_connection(("127.0.0.1", 3030)) as s:
        s.sendall(f"POST / HTTP/1.1\r\nContent-Length: {len(body)}\r\n\r\n{body}".encode())
        resp = s.recv(4096).split(b"\r\n\r\n", 1)[1]
    return json.loads(resp)

print(rpc("balance", {"address": "alice"}))
```

The `--metrics-addr` flag also exposes Prometheus text on a separate socket:

```bash
curl -s 127.0.0.1:9100 | grep mempool_size
```

### Difficulty retargeting test

Exercise the moving-average difficulty algorithm:

```bash
cargo test --test difficulty -- --nocapture
```

### Networking gossip demo

Start three in-process peers and verify longest-chain convergence:

```bash
cargo test --test net_gossip -- --nocapture
```

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
    fee_selector      = 0,
    nonce             = 0,
    memo              = b"hello‑world",
)

stx = sign_tx(sk.encode(), payload)
assert verify_signed_tx(stx)

# 3 Mine a block (CPU PoW)
block = mine_block([stx])  # returns dict‑like Python object
print(block["header"]["hash"])
```

The `fee_selector` chooses which token pool covers the fee: `0` for consumer
tokens, `1` for industrial tokens, or `2` to split evenly. Unless you need a
different funding source, use `0`.

All functions return Python‑native types (`dict`, `bytes`, `int`) for simplicity.

Additional helpers:

- `decode_payload(bytes)` reverses canonical encoding to `RawTxPayload`.
  - `PurgeLoop` provides a context manager that spawns and joins the TTL purge loop.
    `TB_PURGE_LOOP_SECS` must be set to a positive integer; invalid or missing
    values raise ``ValueError``. For manual control use
    `ShutdownFlag`/`PurgeLoopHandle` with `spawn_purge_loop(bc, 1,
    ShutdownFlag())` where the second argument specifies the interval in
    seconds.
- Reusing or skipping a nonce raises `ErrNonceGap`.

### Decoding transaction payloads

Produce canonical bytes for a payload and decode them back:

```python
from the_block import RawTxPayload, canonical_payload, decode_payload

payload = RawTxPayload(
    from_="alice",
    to="bob",
    amount_consumer=5,
    amount_industrial=0,
    fee=10,
    fee_selector=0,
    nonce=0,
    memo=b"demo",
)

raw = canonical_payload(payload)
decoded = decode_payload(raw)
print(decoded.from_, decoded.nonce)
```

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

## Status & Roadmap

### Accomplishments

- **NonceGap enforcement** – admission rejects any transaction that skips a nonce and surfaces a dedicated `ErrNonceGap` exception.
- **Python purge-loop context manager** – `PurgeLoop` wraps `ShutdownFlag` and `PurgeLoopHandle`, spawning the TTL cleanup thread on entry and triggering shutdown/join on exit.
- **Telemetry counter saturation** – `TTL_DROP_TOTAL` and `ORPHAN_SWEEP_TOTAL` saturate at `u64::MAX`; tests assert `ShutdownFlag.trigger()` halts purge threads before overflow.
- **Markdown anchor validation** – `scripts/check_anchors.py --md-anchors` verifies intra-repo section links and runs in CI.
- **Dynamic difficulty retargeting** – proof-of-work adjusts to network timing using a moving average of the last 120 blocks with a ±4× clamp.
- **In-block nonce continuity** – `validate_block` tracks nonces per sender and rejects gaps or repeats within mined blocks.
- **Cross-language serialization determinism** – Rust generates canonical payload CSV vectors and a Python script reencodes them to assert byte equality.
- **Schema v4 migration** – legacy databases recompute coinbase and fee checksums to preserve total supply.
- **Demo narration** – `demo.py` now explains fee selectors, nonce reuse, and automatically manages purge-loop lifetime.
- **Manual purge-loop demo** – setting `TB_DEMO_MANUAL_PURGE=1` exercises an
  explicit `ShutdownFlag`/handle sequence before the context-manager path.
- **Concurrent purge-loop stress tests** – `tests/test_spawn_purge_loop.py`
  proves multiple loops can run and join in any order without panics.
- **CLI node & JSON-RPC** – `cargo run --bin node` serves balance queries,
  transaction submission, mining control, and metrics over JSON-RPC.
- **Minimal P2P gossip** – the `net` module gossips transactions/blocks over
  TCP and adopts the longest chain.
- **Stable admission error codes** – table-driven tests assert each
  `TxAdmissionError` maps to its `ERR_*` constant and telemetry JSON includes a
  numeric `code`.
- **Environment purge metrics** – `tests/test_purge_loop_env.py` drops a
  TTL-expired and orphaned transaction and verifies `ttl_drop_total` and
  `orphan_sweep_total` increments.

### Immediate Priorities (0–2 months)

- Harden admission atomicity by unifying `(sender, nonce)` insert + reservation into a single operation.
- Property tests ensuring pending balance rollbacks and contiguous nonces after drops or reorgs.
- Expand RPC and gossip integration tests to cover adversarial scenarios.

### Medium Term (2–6 months)

- Replace the in-memory `SimpleDb` with a crash-safe backend (sled/RocksDB) behind a storage trait.
- Harden the P2P layer with peer discovery, inventory exchange, and robust fork reconciliation.
- Extend the CLI/RPC surface with authentication and additional admin tooling.

### Long Term (6 months +)

- Research proof-of-service extensions that reward external resource contributions.
- Abstract signature verification to allow pluggable post-quantum algorithms.
- Define an on-chain governance mechanism with feature-bit negotiation for upgrades.

---

## Contribution Guidelines

1. **Fork & branch**: `git checkout -b feat/<topic>`.
2. **Follow coding standards** (`cargo fmt`, Clippy, Black, `python scripts/check_anchors.py --md-anchors`).
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

Use `with PurgeLoop(bc):` to honor `TB_PURGE_LOOP_SECS` and spawn a background
thread that automatically triggers shutdown and joins when the block exits.
`TB_PURGE_LOOP_SECS` sets the interval **in seconds** between TTL sweeps and
must be a positive integer. Unset, non-numeric, or non-positive values raise an
error. For manual control, call `spawn_purge_loop(bc, 1, ShutdownFlag())` to
obtain a `PurgeLoopHandle` you can join explicitly. Dropping the handle
triggers shutdown and joins the thread if you omit an explicit
`ShutdownFlag.trigger()`. The loop periodically
invokes `purge_expired`, trimming TTL-expired entries even without new
submissions and driving `ttl_drop_total` and `orphan_sweep_total`. Counters
saturate at `u64::MAX` to prevent overflow.

### Sample JSON log output

When built with `--features telemetry-json`, log lines include a numeric `code`
for programmatic matching:

```json
{"op":"reject","sender":"a","nonce":3,"reason":"nonce_gap","code":3}
{"op":"purge_loop","reason":"ttl_drop_total","code":0,"fpb":1}
{"op":"purge_loop","reason":"orphan_sweep_total","code":0,"fpb":1}
```

Example:

```bash
TB_PURGE_LOOP_SECS=30 python demo.py
```

If the purge thread panics, calling `PurgeLoopHandle.join()` raises
`RuntimeError`. Set `RUST_BACKTRACE=1` to append a Rust backtrace to the
message:

```bash
RUST_BACKTRACE=1 python - <<'PY'
from the_block import Blockchain, ShutdownFlag, spawn_purge_loop

bc = Blockchain.with_difficulty('demo-db', 1)
handle = spawn_purge_loop(bc, 1, ShutdownFlag())

try:
    handle.join()
except RuntimeError as err:
    print(err)  # backtrace included
PY
```

Admission failures raise typed exceptions. Each `TxAdmissionError` variant
encodes a stable `u16` and is re-exposed to Python both as an `ERR_*`
constant and through the exception's `.code` attribute. Structured telemetry
logs emit the same numeric `code` field alongside the textual `reason` so
downstream consumers can rely on fixed identifiers when parsing rejects.

Key metrics: `mempool_size`, `evictions_total`, `fee_floor_reject_total`,
`dup_tx_reject_total`, `ttl_drop_total`, `startup_ttl_drop_total` (expired mempool entries dropped during startup),
`lock_poison_total`, `orphan_sweep_total`,
`invalid_selector_reject_total`, `balance_overflow_reject_total`,
`drop_not_found_total`, and `tx_rejected_total{reason=*}`. Spans
[`mempool_mutex`](src/lib.rs#L1132-L1145), [`admission_lock`](src/lib.rs#L1596-L1608),
[`eviction_sweep`](src/lib.rs#L1684-L1704), and [`startup_rebuild`](src/lib.rs#L936-L948) annotate
sender, nonce, fee-per-byte, and sweep details.

Report security issues privately via `security@the-block.dev` (PGP key in `docs/SECURITY.md`).

---

## Disclaimer

This software is a production‑grade blockchain kernel under active development. It is not investment advice and comes with no warranty. Use at your own risk.

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
