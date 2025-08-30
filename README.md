## Table of Contents

1. [Why The Block](#why-the-block)
2. [Vision & Current State](#vision--current-state)
3. [Quick Start](#quick-start)
4. [Installation & Bootstrap](#installation--bootstrap)
5. [Build & Test Matrix](#build--test-matrix)
6. [Node CLI and JSON-RPC](#node-cli-and-json-rpc)
7. [Using the Python Module](#using-the-python-module)
8. [Architecture Primer](#architecture-primer)
9. [Project Layout](#project-layout)
10. [Status & Roadmap](#status--roadmap)
11. [Contribution Guidelines](#contribution-guidelines)
12. [Security Model](#security-model)
13. [Telemetry & Metrics](#telemetry--metrics)
14. [Final Acceptance Checklist](#final-acceptance-checklist)
15. [Disclaimer](#disclaimer)
16. [License](#license)

---

## Why The Block

- Dual fee lanes (Consumer | Industrial) with lane-aware mempools and a comfort guard that defers industrial when consumer p90 fees exceed threshold.
- Service credits ledger: non-transferable credits to offset writes (reads are free) and priority with CLI top-up and balance queries.
- Idempotent receipts: compute and storage actions produce stable BLAKE3-keyed receipts for exactly-once semantics across restarts.
- TTL-based gossip relay with duplicate suppression and sqrt-N fanout.
- LocalNet assist receipts earn credits and on-chain DNS TXT records expose gateway policy.
- Rust: `#![forbid(unsafe_code)]`, Ed25519 + BLAKE3, schema-versioned state, reproducible builds.
- PyO3 bindings for rapid prototyping.

## Vision & Current State

### Live now

- 1-second L1 metronome and difficulty retargeting (MA of last 120 blocks, ±4× clamp).
- Dual fee lanes embedded in `SignedTransaction` and lane-specific mempools with p50/p90 fee sampling.
- Industrial admission with capacity estimator, fair-share caps, and burst budgets; structured rejection reasons: `Capacity` | `FairShare` | `BurstExhausted`.
- Storage pipeline with Reed–Solomon erasure coding, multi-provider placement, manifest receipts, reassembly with integrity checks.
- Paid compute-market settlement: credits ledger debits buyers and accrues providers with idempotent BLAKE3-keyed receipts.
- Disk-backed service credits ledger with governance-controlled issuance, decay,
  and per-source expiry.
- Identity handles: normalized, nonce-protected registrations; `register_handle` / `resolve_handle` RPC.
- Governance MVP: propose/vote with delayed activation and single-shot rollback; parameter registry includes snapshot interval & comfort thresholds.
- P2P handshake with feature bits; token-bucket RPC limiter; TTL/orphan purge loop with metrics.
- Devnet swarm tooling with chaos mode; deterministic gossip test with deterministic sleeps and a height→weight→tip-hash tie-break for reproducible convergence.
- Grafana/Prometheus dashboards for snapshot, badge, mempool, admission, gossip convergence, price board.
- WAL fuzzing infra (nightly), F★ installer with caching, formal docs.
- TTL-based gossip relay with duplicate suppression and bounded √N fanout.
- Per-lane mempool stats RPC and `mempool_size{lane}` gauges.
- LocalNet assist receipt submission with replay protection and credit awards.
- On-chain DNS TXT records and `gateway.policy` lookups.
- Provider catalog with RTT/loss probes and background storage repair loop.
- Crash-safe WAL with end-of-compaction marker and replay idempotency keys.

### Planned

- Peer discovery and inventory exchange hardening.

## Quick Start

```bash
# Unix/macOS
bash ./scripts/bootstrap.sh          # installs toolchains, pins cargo-nextest, builds wheel; installs patchelf on Linux
python demo.py               # demo with background purge loop

# Windows (PowerShell)
./scripts/bootstrap.ps1              # run as admin for VS Build Tools
python demo.py
```

Start a node with telemetry and metrics:

```bash
cargo run --features telemetry --bin node -- run \
  --rpc-addr 127.0.0.1:3030 \
  --metrics-addr 127.0.0.1:9100 \
  --mempool-purge-interval 5 \
  --snapshot-interval 600
```

Submit an industrial lane transaction via CLI:

```bash
blockctl tx submit --lane industrial --from alice --to bob --amount 1 --fee 1 --nonce 1
```

Demo assertions against `/metrics` only trigger when built with `--features telemetry`.

Run the deterministic gossip demo:

```bash
cargo nextest run tests/net_gossip.rs
```

This test uses deterministic sleeps and a height→weight→tip-hash tie-break to guarantee reproducible convergence.

Inspect and manage service credits:

```bash
cargo run --bin node -- credits top-up --provider alice --amount 100
cargo run --bin node -- credits balance alice
```

See [`docs/credits.md`](docs/credits.md) for ledger details and additional examples under `examples/governance/CREDITS.md`.

## Installation & Bootstrap

| OS                   | Command                     | Notes |
| -------------------- | --------------------------- | ----- |
| **Linux/macOS/WSL2** | `bash ./scripts/bootstrap.sh`       | prepends `.venv/bin` to `PATH`, creates `bin/python` shim if needed, installs `patchelf` on Linux |
| **Windows 10/11**    | `./scripts/bootstrap.ps1` *(Admin)* | creates `bin/python` shim if needed |

- `build.rs` detects `libpython` via `python3-config --ldflags` and sets rpath; errors early if missing.
- `cargo-nextest` (v0.9.97-b.2) is installed by bootstrap; devs must run `nextest` or the `Justfile` fallback runs `cargo test`.
- Nightly Rust is required only for `cargo fuzz`.
- On Linux only, `patchelf` fixes shared library paths for the built wheel.

## Build & Test Matrix

- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all --features test-telemetry --release`
- `cargo nextest run --all-features compute_market::courier_retry_updates_metrics tests/price_board.rs tests/net_gossip.rs`
- `cargo +nightly fuzz run wal_fuzz -- -max_total_time=60`
- `make -C formal`
- `(cd monitoring && npm ci && make lint)`

CI path-gates monitoring lint on `monitoring/**` changes.

## Node CLI and JSON-RPC

Lane-tagged transaction via RPC:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":1,"method":"tx_submit","params":{"lane":"Industrial","from":"alice","to":"bob","amount":1,"fee":1,"nonce":1}}'
```

Governance RPC:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":2,"method":"gov_propose","params":{"key":"SnapshotIntervalSecs","value":1200,"deadline":12345}}'
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":5,"method":"gov_vote","params":{"id":1,"approve":true}}'
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":6,"method":"gov_params"}'
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":7,"method":"gov_rollback_last"}'
```

Proposals activate after their deadline and only the most recent activation can be rolled back via `gov_rollback_last`.

Identity RPC:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":3,"method":"register_handle","params":{"handle":"@alice","address":"<addr>","nonce":2,"sig":"<hex>"}}'
```

Price board:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":4,"method":"price_board_get"}'
```

Mempool stats per lane:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":8,"method":"mempool.stats","params":{"lane":"Consumer"}}'
```

Submit a LocalNet assist receipt:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":9,"method":"localnet.submit_receipt","params":{"receipt":"<hex>"}}'
```

Publish a DNS TXT record and query gateway policy:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":10,"method":"dns.publish_record","params":{"domain":"example.com","record":{"txt":"policy"},"sig":"<hex>"}}'
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":11,"method":"gateway.policy","params":{"domain":"example.com"}}'
```

Fetch recent micro‑shard roots:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":12,"method":"microshard.roots.last","params":{"n":5}}'
```

Compute courier:

```bash
blockctl courier send slices.json && blockctl courier flush
```

Metrics require `--metrics-addr` and `--features telemetry`.

## Using the Python Module

```python
from the_block import Blockchain

bc = Blockchain.with_difficulty("demo-db", 1)
# lane selection occurs in the signed payload or via fee selector + lane tag
```

Set `PYO3_PYTHON` or `PYTHONHOME` on macOS if the linker cannot find Python.

## Architecture Primer

- Dual fee lanes: lane tag covered by signatures; lane-specific mempools; comfort guard tied to consumer p90 fees.
- Industrial admission: moving-window capacity estimator; fair-share & burst budgets; labeled rejections.
- Storage pipeline: 1 MiB chunks with Reed–Solomon parity; ChaCha20-Poly1305; manifest receipts; integrity verified at read; multi-provider placement.
- Compute market: paid settlement via credits ledger with idempotent receipt tracking.
- Governance MVP: parameter registry with delayed activation & single-shot rollback (keys: `SnapshotIntervalSecs`, `ConsumerFeeComfortP90Microunits`, `IndustrialAdmissionMinCapacity`).
- P2P: feature-bit handshake; token-bucket RPC limiter; purge loop.
- Hashing/signature: Ed25519 + BLAKE3; `#![forbid(unsafe_code)]`.

## Project Layout

```
node/
  src/
    bin/
    compute_market/
    net/
    lib.rs
    ...
  tests/
  benches/
  .env.example
crates/
monitoring/
examples/governance/
examples/workloads/
fuzz/wal/
formal/
scripts/
  bootstrap.sh
  bootstrap.ps1
  requirements.txt
  requirements-lock.txt
  docker/
demo.py
docs/
  compute_market.md
  credits.md
  service_badge.md
  governance_rollback.md
  wal.md
  snapshots.md
  monitoring.md
  formal.md
  detailed_updates.md
AGENTS.md
```

Tests and benches live under `node/`.

If your tree differs, run the repo re-layout task in `AGENTS.md`.

## Status & Roadmap

Progress: ~82/100.

**Recent**

- TTL-based gossip relay with duplicate suppression and bounded fanout metrics.
- Per-lane mempool stats RPC and comfort guard for consumer latency.
- Gateway DNS module with signed TXT records and policy lookups.
- LocalNet assist receipt submission with replay protection and credit awards.
- Provider catalog health checks with automatic storage repair loop.
- Crash-safe WAL with end-of-compaction marker and idempotency keys.
- Credit decay and per-source expiry with governance-controlled issuance.

**Immediate**

- Stabilize `cargo test --all --features test-telemetry --release`.
- Persistence hardening.
- Fuzz coverage expansion.
- Governance docs/API polish.

**Near term**

- Settlement auditing and explorer integration.
- Peer discovery and inventory exchange hardening.

## Contribution Guidelines

- Run both `cargo test` and `cargo nextest run` before opening a PR.
- `cargo fmt`, `cargo clippy`, and fuzz/monitoring checks must be clean.
- See `AGENTS.md` for the Definition of Done and path-gated monitoring lint.

## Security Model

- Domain separation prevents cross-network replay.
- Strict signature verification eliminates malleability.
- No unsafe Rust ensures memory safety.
- Checksummed, deterministic DB protects state integrity.
- Handle registrations are nonce-monotonic and attested; replays rejected.
- Receipt stores use compare-and-swap to guarantee exactly-once persistence.
- WAL fuzz harness runs nightly with seed extraction for triage.

## Telemetry & Metrics

Key counters and gauges:

- `mempool_size{lane}`, `consumer_fee_p50`, `consumer_fee_p90`.
- `admission_mode{mode}`, `industrial_admitted_total`, `industrial_deferred_total`, `industrial_rejected_total{reason}`.
- `gossip_duplicate_total`, `gossip_fanout_gauge`, `gossip_convergence_seconds`, `fork_reorg_total`.
- `credit_issued_total{source}`, `credit_issue_rejected_total{reason}`, `credit_burn_total{sink}`.
- `snapshot_interval_changed`, `badge_active`, `badge_last_change_seconds`.
- `courier_flush_attempt_total`, `courier_flush_failure_total`.
- `storage_put_bytes_total`, `storage_chunk_put_seconds`, `storage_repair_bytes_total`.
- `price_band_p25{lane}`, `price_band_median{lane}`, `price_band_p75{lane}`.

```bash
curl -s 127.0.0.1:9100 | grep -E 'mempool_size|industrial_rejected_total|gossip_convergence_seconds'
```

Metrics are exposed only when the node is started with `--features telemetry` and `--metrics-addr`.

Grafana dashboard panels: snapshot p90, snapshot failures, badge status, mempool occupancy by lane, admission rejections by reason, gossip convergence histogram, price board bands.

Run the stack:

```bash
(cd monitoring && npm ci && make lint)
make monitor   # Prom+Grafana; scrape :9100, open :3000
```

## Final Acceptance Checklist

- README shows the canonical repo layout and `node/` holds tests and benches.
- Commands copy/paste-run after `./scripts/bootstrap.sh` on Linux/macOS and `./scripts/bootstrap.ps1` on Windows.
- RPC names and parameters match the code (lane tags, identity, governance, price board, courier).
- Metric names match exporter output when the node runs with `--features telemetry` and `--metrics-addr`.
- Quick Start node example exposes `/metrics`, and the curl scrape command succeeds.
- Links to `docs/*` and `examples/*` validate via `python scripts/check_anchors.py --md-anchors`.
- Nightly toolchain is required only for `cargo fuzz`.
- macOS rpath guidance for PyO3 (`PYO3_PYTHON`/`PYTHONHOME`) is documented.
- Status & Roadmap states ~82/100 and maps to concrete next tasks.

## Disclaimer

This software is a production-grade blockchain kernel under active development. It is not investment advice and comes with no warranty. Use at your own risk.

## License

Copyright 2025 IJR Enterprises, Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this project except in compliance with the License.
You may obtain a copy of the License at

```
http://www.apache.org/licenses/LICENSE-2.0
```

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
implied. See the [LICENSE](LICENSE) for the specific language
governing permissions and limitations under the License.
