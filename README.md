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
- Inflation-funded storage/read/compute subsidies paid directly in CT with governance-adjustable multipliers.
  Every block carries three explicit subsidy fields—`STORAGE_SUB_CT`,
  `READ_SUB_CT`, and `COMPUTE_SUB_CT`—that top up the miner coinbase based on
  measured work. Usage metrics (bytes stored, bytes served, CPU milliseconds,
  and bytes returned) feed the one‑dial multiplier formula:

  \[
  \text{multiplier}_x =
    \frac{\phi_x I_{\text{target}} S / 365}{U_x / \text{epoch\_secs}}
  \]

  Adjustments are clamped to ±15 % per epoch; if utilisation `U_x` nearly
  vanishes, the previous multiplier doubles to preserve incentive. A global
  kill switch (`kill_switch_subsidy_reduction`) lets governance scale all
  multipliers down during emergencies. This subsidy scheme replaces the
  retired third-token ledger and guarantees operators always receive liquid CT
  for their work without per-request billing or manual swaps.
  The base miner reward follows

  \[
  R_0(N) = \frac{R_{\max}}{1+e^{\xi (N-N^\star)}}
  \]

  with hysteresis `ΔN ≈ √N*` to damp reward oscillation from flash joins and
  leaves.
- Historical context for the transition away from the third token is in [docs/system_changes.md](docs/system_changes.md#2024-third-token-ledger-removal-and-ct-subsidy-transition).
- Idempotent receipts: compute and storage actions produce stable BLAKE3-keyed receipts for exactly-once semantics across restarts.
- TTL-based gossip relay with duplicate suppression and sqrt-N fanout.
- LocalNet assist receipts record proximity attestations and on-chain DNS TXT records expose gateway policy; see [docs/localnet.md](docs/localnet.md) for discovery and session details.
- Rust: `#![forbid(unsafe_code)]`, Ed25519 + BLAKE3, schema-versioned state, reproducible builds.
- PyO3 bindings for rapid prototyping.

## Vision & Current State

### Live now

- Stake-weighted PoS finality with validator registration, bonding/unbonding, and slashing RPCs; stake dictates leader schedule and exits honor delayed unbonding to protect liveness.
- Proof-of-History tick generator and Turbine-style gossip for deterministic block propagation; packets follow a sqrt-N fanout tree with deterministic seeding for reproducible tests.
- Parallel execution engine running non-overlapping transactions across threads; conflict detection partitions read/write sets so independent transactions execute concurrently.
- GPU-optional hash workloads for validators and compute marketplace jobs; GPU paths are cross-checked against CPU hashes to guarantee determinism.
- Modular wallet framework with hardware signer support and CLI utilities; command-line tools wrap the wallet crate and expose key management and staking helpers.
- Cross-chain exchange adapters for Uniswap and Osmosis with fee and slippage checks; unit tests cover slippage bounds and revert on price manipulation.
- Light-client crate with mobile example and FFI helpers; mobile demos showcase header sync, background polling, and optional KYC flows.
- SQLite-backed indexer, HTTP explorer, and profiling CLI; node events and anchors persist to a local database that the explorer queries over REST.
- Distributed benchmark harness and economic simulation modules; harness spawns multi-node topologies while simulators model inflation, fees, and demand curves.
- Installer CLI for signed packages and auto-update stubs; release artifacts include reproducible build metadata and updater hooks.
- Jurisdiction policy packs, governance metrics, and webhook alerts; nodes can load region-specific policies and push governance events to external services.
- Free-read architecture: receipt-only read logging, execution receipts for
  dynamic pages, token-bucket rate limits, governance-seeded reward pools, and
  `gateway.reads_since` analytics. When a client downloads a blob or visits a
  hosted page, the gateway only logs a compact `ReadAck` signed by the client;
  no fee is deducted. Gateways batch these acknowledgements, anchor a Merkle
  root on-chain, and claim the corresponding `READ_SUB_CT` in the next block.
  Dynamic endpoints emit `ExecReceipt` records that capture CPU time and bytes
  out, tying `COMPUTE_SUB_CT` subsidies to verifiable execution. Operators
  should monitor `subsidy_bytes_total{type}` and `subsidy_cpu_ms_total` metrics
  alongside `read_denied_total{reason}` to catch rate-limit abuse or abnormal
  reward patterns.
- Fee-aware mempool with deterministic priority and EIP-1559 style base fee tracking; low-fee transactions are evicted when capacity is exceeded and each block adjusts the base fee toward a fullness target.
- Bridge primitives with relayer proofs and a lock/unlock state machine; `blockctl bridge deposit` and `withdraw` commands move funds across chains while verifying relayer attestations.
- Durable smart-contracts backed by a bincode `ContractStore`; `contract deploy` and `contract call` CLI flows persist code and key/value state under `~/.the_block/state/contracts/` and survive node restarts.
- Persistent DEX order books and trade logs via `DexStore`; order matching updates trust lines atomically and reloads from disk after crashes or upgrades.
- Multi-hop trust-line routing uses cost-based path scoring with fallback routes so payments continue even if a preferred hop disappears mid-flight.
- CT balance and rate-limit push notifications: wallet hooks expose web push/Firebase endpoints and trigger alerts whenever balances change or throttles engage.
- Jittered JSON-RPC client with exponential backoff to avoid thundering herds; timeouts and retry windows are configurable via environment variables.
- Settlement audit task in CI replays recent receipts and fails the build on mismatched anchors to guarantee explorer and ledger consistency.
- Fuzz coverage harness auto-installs `llvm-profdata`/`llvm-cov`, discovers fuzz binaries under the workspace `target` tree, and warns when instrumentation artifacts are missing.
- Operator runbook for manual DHT recovery documents purging peer databases, reseeding bootstrap peers, and verifying network convergence.

### Roadmap

See the [Status & Roadmap](#status--roadmap) section below for recent progress and upcoming tasks.

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

Stake CT for service roles using the wallet helper:

```bash
cargo run --bin wallet stake-role gateway 100 --seed <hex>
# withdraw bonded CT
cargo run --bin wallet stake-role gateway 50 --withdraw --seed <hex>
# query rent-escrow balance
cargo run --bin wallet escrow-balance <account>
```

Subsidy multipliers are governed on-chain via `inflation.params` proposals.

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
- `scripts/fuzz_coverage.sh /tmp/fcov` *(run after generating `.profraw` files via `cargo fuzz` with coverage flags)*
- `cargo test -p the_block --test settlement_audit --release` *(runs receipt verification against the explorer indexer)*

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

Discovery, handshake, and proximity rules are detailed in [docs/localnet.md](docs/localnet.md).

Publish a DNS TXT record and query gateway policy:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":10,"method":"dns.publish_record","params":{"domain":"example.com","record":{"txt":"policy"},"sig":"<hex>"}}'
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":11,"method":"gateway.policy","params":{"domain":"example.com"}}'
```
`gateway.policy` responses include `reads_total` and `last_access_ts` counters.

Fetch recent micro‑shard roots:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":12,"method":"microshard.roots.last","params":{"n":5}}'
```

Query subsidy multipliers and bonded stake:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":13,"method":"inflation.params"}'

curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":14,"method":"stake.role","params":{"id":"<addr>","role":"gateway"}}'
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

- Dual fee lanes: every transaction carries a signed lane tag that feeds lane-specific mempools; a comfort guard monitors consumer p90 fees and defers industrial traffic when congestion rises.
- Industrial admission: a moving-window capacity estimator and fair-share / burst budgets gate high-volume clients; rejected transactions surface explicit reasons for operator tuning.
- Storage pipeline: 1 MiB chunks with Reed–Solomon parity and ChaCha20‑Poly1305 encryption; manifest receipts record chunk hashes and placements, and reads verify integrity against the manifest.
- Free-read architecture: gateways log per-read receipts, batch hourly Merkle roots, anchor them on L1, and replenish providers from governance-seeded reward pools while token-bucket rate limits absorb abuse.
- Compute market: workloads settle via CT balances; idempotent receipts guarantee each compute slice is accounted once even across retries.
- Governance MVP: a parameter registry with delayed activation and single-shot rollback (keys: `SnapshotIntervalSecs`, `ConsumerFeeComfortP90Microunits`, `IndustrialAdmissionMinCapacity`) lets validators tune the network without hard forks.
- P2P: peers handshake with feature bits, enforce token-bucket RPC limits, and run a purge loop to evict stale connections.
- Hashing/signature: Ed25519 keys and BLAKE3 hashes under `#![forbid(unsafe_code)]` deliver a memory-safe, modern cryptographic base.

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

Mainnet readiness: ~94/100 · Vision completion: ~63/100.

The third-token ledger has been fully retired. Every block now mints
`STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` in the coinbase,
with epoch‑retuned `beta/gamma/kappa/lambda` multipliers smoothing
inflation to ≤ 2 %/year. Historical context and migration notes are in
[`docs/system_changes.md`](docs/system_changes.md#2024-third-token-ledger-removal-and-ct-subsidy-transition).

For a subsystem-by-subsystem breakdown with evidence and remaining gaps, see
[docs/progress.md](docs/progress.md).

### Strategic Pillars

| Pillar | % Complete | Highlights | Gaps |
| --- | --- | --- | --- |
| **Governance & Subsidy Economy** | **75 %** | Inflation governors tune β/γ/κ/λ multipliers and rent rate; governance can seed reward pools for service roles. | No on-chain treasury or proposal dependencies; grants and multi-stage rollouts remain open. |
| **Consensus & Core Execution** | 66 % | Stake-weighted leader rotation, deterministic tie-breaks, and parallel executor guard against replay collisions. | Finality gadget lacks rollback stress tests and formal proofs. |
| **Smart-Contract VM & UTXO/PoW** | 42 % | Minimal bytecode engine with gas metering and BLAKE3 PoW headers. | No persistent storage, deployment flow, or opcode library parity. |
| **Storage & Free-Read Hosting** | **76 %** | Receipt-only logging, hourly batching, L1 anchoring, and `gateway.reads_since` analytics keep reads free yet auditable. | Incentive-backed DHT storage and offline reconciliation remain prototypes. |
| **Compute Marketplace & CBM** | 60 % | GPU/CPU workloads emit deterministic `ExecutionReceipt`s and redeem via compute-backed money curves. | No heterogeneous scheduling or reputation system; SLA arbitration limited. |
| **Trust Lines & DEX** | 45 % | Authorization-aware trust lines and slippage-checked order books support multi-hop payments. | Cost-based path scoring and on-ledger escrow absent. |
| **Cross-Chain Bridges** | 5 % | Design stubs note lock/unlock and relayer incentives. | No contract implementation or safety proofs. |
| **Wallets, Light Clients & KYC** | 69 % | CLI and hardware wallet support, mobile light-client SDKs, and pluggable KYC hooks. | Remote signer, multisig, and production-grade mobile apps outstanding. |
| **Monitoring, Debugging & Profiling** | 66 % | Prometheus/Grafana dashboards expose read-denial and subsidy counters; CLI debugger and profiling utilities ship with nodes. | Bridge/VM metrics and automated anomaly detection missing. |
| **Economic Simulation & Formal Verification** | 33 % | Bench harness simulates inflation/demand; chaos tests capture seeds. | Sparse scenario library and no integrated proof pipeline. |
| **Mobile UX & Contribution Metrics** | 50 % | Background sync and contribution counters respect battery/network constraints. | Push notifications and broad hardware testing pending. |

### Immediate

- Finalize gossip longest-chain convergence, run the chaos harness with 15 % packet loss/200 ms jitter, and document tie-break algorithms and fork-injection fixtures in `docs/networking.md`.
- Retune subsidy multipliers through validator votes (`node/src/rpc/governance.rs`) and expand documentation on inflation parameters.
- Expand settlement audit coverage: index receipts in the explorer (`explorer/indexer.rs`), schedule CI verification jobs, surface mismatches via Prometheus alerts, and ship sample audit reports.
- Harden DHT bootstrapping by persisting peer databases, fuzzing identifier exchange, randomizing bootstrap peer selection, and documenting recovery procedures.
- Broaden fuzz and chaos testing across gateway and storage paths, bound `SimpleDb` bytes to simulate disk-full scenarios, and randomize RPC timeouts for resilience.
- Implement the free-read architecture across gateway and storage: log receipts without charging end users, replenish gateway balances via CT inflation subsidies rather than the retired `read_reward_pool`, enforce token buckets, emit `ExecutionReceipt`s, and update docs/tests to reflect the model. See [system_changes.md](docs/system_changes.md#2024-third-token-ledger-removal-and-ct-subsidy-transition) for historical context.

### Near Term

- **Industrial lane SLA enforcement and dashboard surfacing** – enforce deadline slashing for tardy providers, track ETAs and on-time percentages, visualize payout caps, and ship operator remediation guides.
- **Range-boost mesh trials and mobile energy heuristics** – prototype BLE/Wi-Fi Direct relays, tune lighthouse multipliers via field energy usage, log mobile battery/CPU metrics, and publish developer heuristics.
- **Economic simulator runs for emission/fee policy** – parameterize inflation/demand scenarios, run Monte Carlo batches via bench-harness, report top results to governance, and version-control scenarios.
- **Compute-backed money and instant-app groundwork** – define redeem curves for CBM, prototype local instant-app execution hooks, record resource metrics for redemption, test edge cases, and expose CLI plumbing.

### Medium Term

- **Full cross-chain exchange routing** – implement adapters for SushiSwap and Balancer, integrate bridge fee estimators and route selectors, simulate multi-hop slippage, watchdog stuck swaps, and document guarantees.
- **Distributed benchmark network at scale** – deploy harness across 100+ nodes/regions, automate workload permutations, gather latency/throughput heatmaps, generate regression dashboards, and publish tuning guides.
- **Wallet ecosystem expansion** – add remote signer and multisig modules, ship Swift/Kotlin SDKs, enable hardware wallet firmware updates, provide backup/restore tooling, and host interoperability tests.
- **Governance feature extensions** – roll out staged upgrade pipelines, support proposal dependencies and queue management, add on-chain treasury accounting, offer community alerts, and finalize rollback simulation playbooks.
  - **Mobile light client productionization** – optimize header sync/storage, add push notification hooks for subsidy events, integrate background energy-saving tasks, support mobile signing, and run a cross-hardware beta program.

### Long Term

- **Smart-contract VM and SDK release** – design a deterministic instruction set with gas accounting, ship developer tooling and ABI specs, host example apps, audit and formally verify the stack.
- **Permissionless compute marketplace** – integrate heterogeneous GPU/CPU scheduling, enable provider reputation scoring, support escrowed cross-chain payments, build an SLA arbitration framework, and release marketplace analytics.
- **Global jurisdiction compliance framework** – publish regional policy packs, add PQ encryption, maintain transparency logs, allow per-region feature toggles, and run forkability trials.
- **Decentralized storage and bandwidth markets** – incentivize DHT storage, reward long-range mesh relays, integrate content addressing, benchmark large file transfers, and provide retrieval SDKs.
- **Mainnet launch and sustainability** – lock protocol parameters via governance, run multi-phase audits and bug bounties, schedule staged token releases, set up long-term funding mechanisms, and establish community maintenance committees.

### Next Tasks

1. **Add rollback tests for PoS finality gadget**  
   - Create adversarial forks in `node/tests/finality_rollback.rs`.  
   - Simulate conflicting blocks and ensure the finality gadget reorgs correctly.  
   - Assert ledger state consistency after rollback.
2. **Implement contract deployment and persistent state**  
   - Add deployment transactions in `vm/src/tx.rs` with on-chain storage.  
   - Persist contract key/value state under `state/contracts/`.  
   - Write execution tests confirming state survives restarts.
3. **Ship ABI tooling and contract CLI**  
   - Generate ABI files from `vm/src/opcodes.rs`.  
   - Expose `contract deploy/call` commands in `cli/`.  
   - Document usage in `docs/contract_dev.md`.
4. **Introduce dynamic gas fee market**  
   - Track base fee per block in `node/src/fees.rs`.  
   - Implement EIP-1559-style adjustment based on block fullness.  
   - Update mempool to reject under-priced transactions.
5. **Bridge UTXO and account models**  
   - Add translation layer in `ledger/src/utxo_account.rs`.  
   - Ensure spent UTXOs update account balances atomically.  
   - Provide migration tools for existing balances.
6. **Merge PoW blocks into PoS finality path**  
   - Allow PoW headers to reference PoS checkpoints in `consensus/src/pow.rs`.  
   - Update fork-choice to prefer finalized PoS chain with valid PoW.  
   - Add regression tests covering mixed PoW/PoS chains.
7. **Build fee-aware mempool**  
   - Sort pending transactions by effective fee in `node/src/mempool.rs`.  
   - Evict low-fee transactions when capacity exceeds threshold.  
   - Ensure higher-fee transactions are processed first in tests.
8. **Implement lock/unlock bridge primitives**  
   - Define bridge contracts in `bridges/` for asset locking.  
   - Add relayer proofs verifying remote chain events.  
   - Provide CLI commands for deposit and withdraw.
9. **Persist DEX order books and trades**  
   - Store order books in `dex/src/storage.rs` backed by `SimpleDb`.  
   - Log executed trades in `dex/trades/` for audits.  
   - Recover books after node restart in tests.
10. **Enhance multi-hop trust-line routing**  
   - Implement cost-based path scoring in `trust_lines/src/path.rs`.  
   - Add fallback routes when optimal paths fail mid-transfer.  
   - Update documentation with routing algorithm details.
11. **Expose subsidy parameter proposals in gov-ui**
    - List multiplier and rent-rate proposals in the UI.
    - Allow voting and activation through web interface.
    - Sync results via `governance/params.rs`.
12. **Index settlement receipts in explorer storage**  
   - Parse receipt files in `explorer/indexer.rs`.  
   - Persist anchors and issuance events into explorer DB.  
   - Add REST endpoints to query finalized batches.
13. **Schedule settlement verification in CI**  
   - Add a CI job invoking `settlement.audit`.  
    - Fail builds on mismatched anchors or subsidy totals.
   - Provide sample configs in `ci/settlement.yml`.
14. **Fuzz peer identifier parsing**  
   - Create fuzz target for `net/discovery.rs` identifier parser.  
   - Integrate with `cargo-fuzz` under `fuzz/`.  
   - Run in CI with crash-on-error.
15. **Document manual DHT recovery procedures**  
   - Write `docs/dht_recovery.md` with step-by-step commands.  
   - Include troubleshooting for stale peer lists.  
   - Cross-reference `net/discovery.rs` comments.
16. **Integrate gateway fuzzing**  
   - Build fuzz harness for `gateway/http.rs` request handling.  
   - Seed with realistic HTTP traffic patterns.  
   - Wire into nightly CI runs.
17. **Simulate disk exhaustion in storage tests**  
   - Modify `node/tests/storage_repair.rs` to limit tmpfs size.  
   - Validate graceful error and recovery paths.  
   - Ensure receipts and ledger updates remain consistent.
18. **Randomize RPC client timeouts**  
   - Introduce jitter in `node/src/rpc/client.rs` timeout settings.  
   - Expose config knob `rpc.timeout_jitter_ms`.  
   - Test under high latency to confirm resilience.
19. **Add push notification hooks for subsidy events**
    - Emit webhook or FCM triggers in wallet tooling.
    - Allow mobile clients to register tokens via RPC.
    - Document opt-in flow in `docs/mobile.md`.
20. **Set up formal verification for consensus rules**  
   - Translate the state machine into F* modules under `formal/consensus`.  
   - Create CI jobs running `fstar` to ensure proofs compile.  
   - Provide developer guide in `formal/README.md`.



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
  - `subsidy_bytes_total{type}`, `subsidy_cpu_ms_total`.

- `snapshot_interval_changed`, `badge_active`, `badge_last_change_seconds`.
- `courier_flush_attempt_total`, `courier_flush_failure_total`.
- `storage_put_bytes_total`, `storage_chunk_put_seconds`, `storage_repair_bytes_total`.

See [docs/economics.md](docs/economics.md#epoch-retuning-formula) for the subsidy retuning formula and ROI guidance.
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
- Status & Roadmap states ~94/100 and ~63/100 vision completion and maps to concrete next tasks.

## Disclaimer
