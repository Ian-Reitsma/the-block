## Table of Contents

1. [What is The Block?](#what-is-the-block)
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

> **Review (2025-09-25):** Refreshed readiness metrics and pillar percentages after the fallback coder rollout audit.

## What is The Block?

The Block is a decentralized compute and storage network that blends a
traditional proof-of-work blockchain with a proof-of-service layer. Nodes mint
currency by delivering verifiable storage, bandwidth, and CPU time rather than
solely expending electricity on hashes. Operators publish content and APIs
directly through gateways, while clients retrieve data for free because reads
are logged as signed acknowledgements instead of per-request payments. Every
component, from the gossip layer to the storage pipeline, is written in Rust
with `#![forbid(unsafe_code)]` to guarantee memory safety.

The system was designed to make decentralization practical for everyday
applications, not just token transfers. Small websites and mobile apps can
anchor their assets on-chain and rely on the network to serve users at the edge
without negotiating hosting contracts. Receipts and Merkle proofs allow
auditors to verify that gateways actually delivered bytes and CPU cycles before
claiming subsidies in the next block. Developer tooling—from a CLI node to PyO3
bindings—lets engineers prototype features without spinning up bespoke
infrastructure. The protocol prioritizes determinism so that results are
repeatable across hardware and operating systems.

For the canonical subsystem evidence and live percentages, see
[`docs/progress.md`](docs/progress.md); the summary below highlights the most
operator-facing pieces.

Economics are structured to reward useful work and keep fees predictable. Three
per-block subsidies (`STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT`) mint
liquid CT based on the amount of storage, bandwidth, and compute delivered.
Multipliers retune each epoch to keep inflation under two percent per year, and
governance can throttle all rewards with a kill switch during emergencies. Base
miner rewards follow a logistic curve so that supply does not explode when new
operators join en masse. These mechanisms operate entirely within the unified
CT subsidy ledger and align incentives without per-request billing.

From a user’s perspective, The Block behaves like a resilient, open cloud. A
wallet or light client syncs headers, resolves DNS-like names published on
chain, and requests content without worrying about micro-payments or captive
platforms. Cross-chain bridges and trust lines extend liquidity to other
networks, while contract support lays groundwork for on-chain applications. A
rich telemetry stack exposes metrics for everything from subsidy payouts to
gossip latency, helping operators fine-tune performance. The project remains
experimental but has enough pieces wired together for developers to build and
test real services today.

- On-chain storage and free-read hosting let gateways serve files, websites,
  and API responses without charging end users; signed `ReadAck` receipts
  anchor activity on-chain and mint the corresponding `READ_SUB_CT` reward.
  Reputation-weighted Lagrange coding distributes shards while
  proof-of-retrievability challenges penalize missing data.
  Snapshot rewrites now stage column families through fsync’d temporary files
  before atomically renaming base64 snapshots, preserving legacy dumps until
  the new image lands. The mobile gateway cache persists ChaCha20-Poly1305–
  encrypted responses and offline transactions in a sled store, drains a
  min-heap of expirations every sweep window, rebuilds queues on restart, and
  exposes `mobile_cache_*` counters plus CLI/RPC status & flush endpoints so
  operators can tune TTLs, entry caps, and eviction health. See
  [docs/read_receipts.md](docs/read_receipts.md),
  [docs/simple_db.md](docs/simple_db.md), and
    [docs/mobile_gateway.md](docs/mobile_gateway.md) for the batching, audit,
    persistence, and cache hygiene flow. Storage backends now route through the
    unified `storage_engine` crate so SimpleDb selects RocksDB, sled, or in-memory
    engines via configuration without leaking provider APIs while the shared
    `coding` crate fronts encryption, erasure, fountain, and compression
    primitives. XOR parity and RLE compression fallbacks sit behind explicit
    rollout gates, emit coder/compressor labels on storage latency/failure
    metrics, and feed a bench harness comparison command so operators can quantify
    performance before flipping the switch during dependency incidents. (92.0%
    Complete —
    incentive-marketplace wiring remains the main open track.)
- The compute marketplace pays nodes for deterministic CPU and GPU work
  metered in normalized compute units. Offers escrow CT via the `pct_ct` selector
  (policy now pins it to 100 for live lanes), supports graceful job cancellation
  through the `compute.job_cancel` RPC and `compute cancel <job_id>` CLI, hashes
  receipts into blocks before conversion to CT through multipliers, and verifies
  optional SNARK receipts prior to crediting payment. Settlement persists CT
  balances in a RocksDB-backed ledger with activation metadata, audit exports,
  and recent root tracking so operators can reconcile receipts after restarts;
  `Settlement::shutdown` now forces a final `persist_all` + RocksDB flush so
  regression suites can assert clean teardown, and metadata captures the last
  anchor hash plus cancellation reason for post-incident reviews. Admission now
  enforces a dynamic fee floor with per-sender slot limits, records evictions for
  audit, and exposes the active floor through `mempool.stats` so operators can
  reason about QoS. Governance can retune the fee-floor window and percentile,
  and wallet sends surface localized warnings with auto-bump or `--force`
  overrides plus JSON output for tooling. Lane-aware batching now stages matches
  per `FeeLane`, rotates lanes until the batch quota or fairness deadline trips,
  throttles via `TB_COMPUTE_MATCH_BATCH`, and persists receipts with lane tags so
  restarts replay only outstanding work. The matcher rejects seeds that exceed
  per-lane capacity, tracks starvation thresholds with structured warnings, and
  exports per-lane queue depth/age plus `matches_total{lane}` and
  `match_loop_latency_seconds{lane}` histograms. CLI and RPC surfaces expose queue
  depths, capacity guardrails, fairness windows, and recent matches, and
    settlement continues to persist CT balances with activation metadata, audit
    exports, and recent root tracking. (94.6% Complete)
      - Networking exposes per-peer rate-limit telemetry and drop-reason statistics,
        letting operators run `net stats`, filter by reputation or drop reason, emit
        JSON via `--format json`, and paginate large sets with `--all --limit --offset`.
        A cluster-wide `metrics-aggregator` rolls up `cluster_peer_active_total` and
        `aggregator_ingest_total` gauges, partition markers flag split-brain events,
        and metrics are bounded by `max_peer_metrics` so abusive peers cannot exhaust
        memory. QUIC now derives mutual-TLS certificates from node keys, gossips
        fingerprints, exposes cached diagnostics over the `net.quic_stats` RPC / CLI,
        and leverages a chaos harness to publish retransmit counters, keeping
        operators ahead of packet loss while the transport layer advertises provider metadata, per-provider connect counters, and mockable adapters for tests. Overlay discovery, uptime tracking, and persistence now flow through the
        `p2p_overlay` crate with selectable libp2p/stub backends, CLI overrides,
        telemetry gauges, and integration tests, so node modules only depend on overlay traits. Shard-aware peer maps route block gossip only
        to interested peers and uptime-based fee rebates reward high-availability
        peers. (97.3% Complete)
      - Hybrid proof-of-work and proof-of-stake consensus schedules leaders by stake,
        resolves forks deterministically, and validates blocks with BLAKE3 hashes,
        multi-window Kalman retargeting, VDF-anchored randomness, macro-block
        checkpointing, and per-shard fork choice. Release installs now gate on
        provenance verification with automated rollback if hashes drift, keeping
        consensus nodes in lockstep. (92.7% Complete)
  - Governance and subsidy economics use on-chain proposals to retune `beta`,
    `gamma`, `kappa`, and `lambda` multipliers each epoch, keeping inflation under
    two percent while funding service roles. Release upgrades now require
    multi-signature attestation with persisted signer sets, explorer history,
    and CLI tooling, while the fetcher verifies provenance before installs and
    records rollout timestamps. Fee-floor policy updates persist into
    `GovStore` history with rollback support, telemetry counters, and explorer
    timelines so operators can audit parameter changes while governance history
    archives DID revocations for audit. All tooling now targets the shared
    `governance` crate with sled-backed persistence, proposal DAG validation,
    and Kalman retune helpers, plus durable proof-rebate receipts wired into coinbase assembly. (95.3% Complete)
    - The smart-contract VM couples a minimal bytecode engine with UTXO and account
      models, adds deterministic WASM execution with a debugger, and enables
      deployable contracts and fee markets alongside traditional PoW headers. (85.5%
      Complete)
- Trust lines and the decentralized exchange route multi-hop payments through
  cost-based paths and slippage-checked order books, enabling peer-to-peer
  liquidity. On-ledger escrow and partial-payment proofs now lock funds until
  settlements complete, telemetry gauges `dex_escrow_locked`,
    `dex_escrow_pending`, and `dex_escrow_total` track utilisation, and
    constant-product AMM pools provide fallback liquidity with programmable incentives. (84.1%
    Complete)
  - Cross-chain bridge primitives track per-asset channels, persist relayer sets,
    enforce multi-signature quorums, and expose challenge windows with slashing for
    invalid proofs. Deposit/withdraw flows carry partition tags, HTLC parsing accepts
    both SHA3 and RIPEMD encodings, and light-client verification guards every transfer.
    CLI/RPC surfaces list pending withdrawals, quorum composition, and dispute history.
    (79.4% Complete)
- The decentralized identifier registry anchors DID documents with replay
  protection, optional provenance attestations, and telemetry (`did_anchor_total`).
  Explorer APIs `/dids`, `/identity/dids/:address`, and `/dids/metrics/anchor_rate`
  surface history and anchor velocity, while the `contract light-client did`
  subcommands handle anchoring, resolving, remote signing, and sign-only payload
  export with localized messaging. Governance revocations block misused
  identifiers and are archived alongside anchor history for audit. Explorer pagination
  caches and CLI tooling consume the same data, keeping dashboards aligned with wallet
  history. (82.7% Complete)
    - Wallets, light clients, and optional KYC hooks provide desktop and mobile
      users with secure key management, staking tools, remote signer support,
      session-key derivation, auto-update orchestration, and compliance options as
      needed. Explorer and CLI tooling now surface release history and per-node
      installs for operators, while the wallet caches fee-floor queries, warns
      when sends fall below the floor, and supports localized prompts, JSON
      output, and remote signer attestations. Wallet QoS events feed telemetry so
      dashboards track warning/override deltas. Wallet binaries now share a single
      `ed25519-dalek 2.2.x` stack, emit escrow hash algorithms, forward
        multisig signer sets end-to-end, and expose remote signer telemetry so explorer tooling can validate threshold
        staking payloads. (95.4% Complete)
      - Monitoring, debugging, and profiling tools export Prometheus metrics,
        structured traces, readiness endpoints, VM trace counters, partition dashboards,
        and a cluster-wide `metrics-aggregator` for fleet visibility. Correlation IDs
        now link metrics anomalies to log searches, automated QUIC dumps, and Grafana
        drill-downs for rapid mitigation. Wallet fee-floor overrides and DID
        anchor totals land in telemetry so dashboards can trace user choices,
        anchor velocity, and governance parameter rollbacks from a single pane.
        Overlay gauges (`overlay_backend_active`, `overlay_peer_total`, persisted counts)
        now expose backend health alongside transport for quick verification. (94.2%
        Complete)
  - Economic simulation and formal verification suites model inflation scenarios
    and encode consensus invariants, laying groundwork for provable safety. (42.2%
    Complete)
- Mobile UX and contribution metrics track background sync, battery impact, and
  subsidy events to make participation feasible on phones. Device heuristics now
  integrate platform power/network probes, cache asynchronous readings with
  freshness labels, stream `the_block_light_client_device_status{field,freshness}`
  telemetry, embed snapshots into compressed log uploads, and surface CLI/RPC
  gating messages alongside persisted overrides in `~/.the_block/light_client.toml`.
  Operators can toggle charging/Wi‑Fi requirements via `contract light-client
  device ...` commands, inspect cached readings, and rely on desktop builds that
  fall back to configured defaults without stalling sync. (72.6% Complete)

## Vision & Current State

  Mainnet readiness sits at **98.3/100** with vision completion **90.4/100**.
  Recent work finished wiring the storage pipeline through the `coding` crate so
  every manifest records encryptor, erasure, fountain, and compressor choices.
  Fallback XOR parity and RLE compression now ride behind explicit rollout gates,
  surface algorithm labels in telemetry, and feed the bench harness comparison
  tooling so operators can quantify trade-offs before cutting over during
  incidents. Lane-aware batching in the compute matcher gained fairness
  deadlines, per-lane queue caps, starvation warnings, and
  `match_loop_latency_seconds{lane}` histograms; the gossip relay layers
  LRU-backed deduplication, adaptive fanout, partition tagging, and shard-aware
  persistence; and the proof-rebate pipeline persists receipts to disk, exposes
  explorer/CLI pagination, and feeds coinbase assembly. Governance tooling,
  wallet telemetry, and the resilient RPC client continue to anchor the ecosystem
  on the shared state machine while the bridge stack enforces multi-signature
  quorums, challenge windows, and relayer slashing. Current focus areas: deliver
  treasury disbursement tooling, wire SLA slashing dashboards on top of the new
  matcher, finish the remaining crypto/coding wrapper migrations, continue
  WAN-scale QUIC chaos drills with mitigation playbooks, polish multisig wallet
  UX, and expand bridge/DEX docs with signer-set payloads before the next release
  tag.

### Live now

- Stake-weighted PoS finality with validator registration, bonding/unbonding, and slashing RPCs; stake dictates leader schedule and exits honor delayed unbonding to protect liveness.
- Proof-of-History tick generator and Turbine-style gossip for deterministic block propagation; packets follow a sqrt-N fanout tree with deterministic seeding for reproducible tests. Duplicate suppression and adaptive fanout are detailed in [docs/gossip.md](docs/gossip.md).
- Kalman multi-window difficulty retune keeps the 1 s block cadence stable and is exposed via `consensus.difficulty` RPC, `retune_hint` headers, and `difficulty_*` metrics.
- Parallel execution engine running non-overlapping transactions across threads; conflict detection partitions read/write sets so independent transactions execute concurrently. See [docs/scheduler.md](docs/scheduler.md).
- GPU-optional hash workloads for validators and compute marketplace jobs; GPU paths are cross-checked against CPU hashes to guarantee determinism.
- Compute-market jobs quote normalized compute units and escrow CT via `pct_ct` (live lanes pin the selector to 100). Refunds honour the submitted split, jobs respect lane-aware batching with fairness windows and starvation detection, and operators can inspect per-lane queue depth, capacity limits, and recent matches via CLI/RPC. Background loops throttle with `TB_COMPUTE_MATCH_BATCH`, persist receipts with lane tags, and surface telemetry for dashboards while `compute cancel <job_id>` keeps graceful cancellation intact.
- Cluster-wide `metrics-aggregator` collects peer snapshots while the `net stats`
  CLI supports JSON output, drop-reason and reputation filtering, pagination, and
  colorized drop-rate warnings.
- Node CLI binaries honour feature flags so telemetry, gateway, and QUIC stacks
  only link when explicitly requested. `--auto-tune` now emits a descriptive
  error unless telemetry is enabled, `--metrics-addr` and `--status-addr` fail
  fast when their features are absent, and jurisdiction policy packs record the
  loaded language in law-enforcement audit logs. Feature-light builds therefore
  stay lean without sacrificing operator ergonomics when the full stack is
  required.
- Metrics-to-logs correlation links Prometheus anomalies to targeted log searches,
  automated QUIC dumps, and Grafana deep links for rapid mitigation.
- Partition watch tracks peer reachability and stamps gossip with markers so
  splits can reconcile deterministically once connectivity returns.
- Modular wallet framework with hardware and remote signer support; command-line tools wrap the wallet crate and expose key management and staking helpers. The `contract wallet send` flow caches fee-floor lookups, emits localized warnings when the user fee is below governance policy, offers `--auto-bump` and `--force` paths, and streams telemetry for overrides.
- Pluggable account abstraction with expiring session keys and
  meta-transaction tooling.
- Cross-chain exchange adapters for Uniswap and Osmosis with fee and slippage checks; unit tests cover slippage bounds and revert on price manipulation.
- Versioned P2P handshake negotiates feature bits, records peer metadata, and enforces minimum protocol versions. See [docs/p2p_protocol.md](docs/p2p_protocol.md).
- QUIC gossip transport with mutual-TLS certificate rotation, fingerprint gossip,
  cached diagnostics via `net.quic_stats`/`blockctl net quic stats`, and TCP fallback; fanout selects
  per-peer transport while chaos tooling surfaces retransmit spikes and the metrics-to-logs pipeline dumps offending sessions automatically.
- Light-client crate with mobile example and FFI helpers; mobile demos showcase header sync, background polling, and optional KYC flows. The synchronization model and security trade-offs are described in [docs/light_client.md](docs/light_client.md).
- SQLite-backed indexer, HTTP explorer, and profiling CLI; node events and anchors persist to a local database that the explorer queries over REST. DID anchors feed a dedicated `did_records` table, REST endpoints (`/dids`, `/identity/dids/:address`, `/dids/metrics/anchor_rate`), and an explorer view for cross-navigation with wallet addresses.
- Incremental log indexer tracks ingest offsets, supports encrypted key rotation,
  serves REST/WebSocket searches, and ships with CLI tooling for live correlation.
- Explorer release timeline API, schema, and CLI surfacing proposer addresses,
  signer sets, thresholds, and install counts for governance audits.
- Distributed benchmark harness and economic simulation modules; harness spawns multi-node topologies while simulators model inflation, fees, and demand curves.
- Installer CLI for signed packages and attested auto-updates; the fetcher
  verifies multi-sig provenance, records install timestamps, and rolls back on
  hash drift while release artifacts include reproducible build metadata.
- Jurisdiction policy packs, governance metrics, and webhook alerts; nodes can load region-specific policies and push governance events to external services.
- Law-enforcement portal with hashed case logs and warrant canaries; operators export requests or verify canary freshness without revealing identifiers. See [docs/le_portal.md](docs/le_portal.md).
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
- Blob root scheduler separates ≤4 GiB L2 blobs from larger L3 blobs, flushing roots on 4 s and 16 s cadences to bound anchoring latency. Storage pipelines enqueue roots via `BlobScheduler`; see [docs/blob_chain.md](docs/blob_chain.md).
- Range-boost store-and-forward queue tracks bundles with hop proofs so offline relays can ferry data until connectivity returns. See [docs/range_boost.md](docs/range_boost.md).
- Fee-aware mempool with deterministic priority and EIP-1559 style base fee tracking; low-fee transactions are evicted when capacity is exceeded and each block adjusts the base fee toward a fullness target.
- Admission pipeline enforces per-sender slot limits, records evictions for audit,
  and surfaces the dynamic fee floor via `mempool.stats`.
- Transaction lifecycle document covers payload fields, memo handling, Python bindings, and lane-tagged admission; see [docs/transaction_lifecycle.md](docs/transaction_lifecycle.md).
- Bridge primitives with light-client verification, relayer proofs, and a lock/unlock state machine; `blockctl bridge deposit` and `withdraw` commands move funds across chains while verifying relayer attestations.
- Durable smart-contracts backed by a bincode `ContractStore`; `contract deploy` and `contract call` CLI flows persist code and key/value state under `~/.the_block/state/contracts/` and survive node restarts.
- Deterministic WASM runtime with fuel-based metering and an interactive
  debugger for opcode-level traces.
- Persistent DEX order books and trade logs via `DexStore`; on-ledger escrow and partial-payment proofs lock funds until settlement, and gauges `dex_escrow_locked`/`dex_escrow_pending`/`dex_escrow_total` track funds and counts. Multi-hop trust-line routing uses cost-based path scoring with fallback routes so payments continue even if a preferred hop disappears mid-flight. See [docs/dex.md](docs/dex.md).
- WAL-backed `SimpleDb` provides a lightweight key-value store with crash-safe
  replay and optional byte quotas. DNS caches, chunk gossip, and DEX storage
  all build on this primitive; see [docs/simple_db.md](docs/simple_db.md).
- Gateway DNS publishing exposes signed TXT records and per-domain read counters for free-read auditing. Domains outside the
  chain-specific `.block` TLD require a matching TXT record in the public zone
  before clients honor them. See [docs/gateway_dns.md](docs/gateway_dns.md).
- Durable compute courier records bundles with exponential backoff retries; see [docs/compute_market_courier.md](docs/compute_market_courier.md).
- Macro-block checkpoints capture per-shard roots and inter-shard queue proofs for cross-shard ordering; see [docs/macro_block.md](docs/macro_block.md).
- Real-time state streaming over WebSockets keeps light clients current with zstd-compressed snapshots; see [docs/light_client_stream.md](docs/light_client_stream.md).
- SNARK-verified compute receipts tie payments to Groth16 proofs generated from small WASM tasks; see [docs/compute_snarks.md](docs/compute_snarks.md).
- Reputation-weighted, Lagrange-coded storage allocation with proof-of-retrievability challenges; see [docs/storage_market.md](docs/storage_market.md).
- Constant-product AMM pools with epoch-based liquidity mining incentives; see [docs/dex_amm.md](docs/dex_amm.md).
- Network fee rebates reward high-uptime peers via `peer.rebate_status` RPC and `net rebate claim`; see [docs/fee_rebates.md](docs/fee_rebates.md).
- Build provenance checks hash the running binary, verify SBOM signatures, and expose `version provenance` and offline `provenance-verify` tooling; see [docs/provenance.md](docs/provenance.md).
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
# optional QUIC + difficulty demo
python demo.py --quic        # spawns a node with QUIC and prints live difficulty

The optional mode launches a node subprocess, begins mining, and polls
`consensus.difficulty` over JSON‑RPC. Any retarget adjustments are
printed to stdout. Supplying `--quic` enables the QUIC listener so peer
connections can upgrade from TCP.

See [docs/demo.md](docs/demo.md) for a detailed walkthrough of the demo and its
output.

For production deployment, QUIC configuration, and difficulty monitoring, see
[docs/operators/run_a_node.md](docs/operators/run_a_node.md).

# Windows (PowerShell)
./scripts/bootstrap.ps1              # run as admin for VS Build Tools
python demo.py
```

Start a node with telemetry and metrics:

```bash
AGGREGATOR_AUTH_TOKEN=secret \
cargo run --features telemetry --bin node -- run \
  --rpc-addr 127.0.0.1:3030 \
  --metrics-addr 127.0.0.1:9100 \
  --metrics-aggregator-url http://127.0.0.1:9101 \
  --mempool-purge-interval 5 \
  --snapshot-interval 600
```

Submit an industrial lane transaction via CLI:

```bash
blockctl tx submit --lane industrial --from alice --to bob --amount 1 --fee 1 --nonce 1
```

Cancel a job and roll back resources:

```bash
blockctl compute cancel <job_id>
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

Current PoW difficulty:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":8,"method":"consensus.difficulty"}'
```

Run `cargo run -p the_block --example difficulty` to poll the endpoint and
observe difficulty adjustments as blocks are mined.

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

### Compute Marketplace Cancellations

Gracefully stop a running job and refund locked fees:

```bash
# cancel a job via CLI
blockctl compute cancel <job_id>

# equivalent JSON-RPC call
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":16,"method":"compute.job_cancel","params":{"job_id":"<hex>","reason":"client"}}'
```

Successful cancellations free scheduler slots, roll back courier state, and
increment `scheduler_cancel_total{reason}`. Providers may take a reputation hit
depending on the supplied reason.

Inspect per-peer metrics:

```bash
# table output for one peer
blockctl net stats <peer_id>

# JSON output filtered by drops and reputation
blockctl net stats --drop-reason throttle --min-reputation 0.4 --format json

# paginate through the full set
blockctl net stats --all --limit 50 --offset 50

# export and reset metrics
blockctl net stats export <peer_id> --path /tmp/peer.json
blockctl net stats reset <peer_id>

# generate bash completions
blockctl net completion bash > /etc/bash_completion.d/blockctl-net

# equivalent RPC
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":15,"method":"net.peer_stats","params":{"peer_id":"<hex>"}}'
```

Sample table output ends with a summary line:

```
PEER          REQS  DROPS  RATE
12D3KooW...     10      1   10%
---
1 peer (1 active)
```

Rows turn yellow when drop rate exceeds 5 % and red at 20 %. The command exits
with `0` on success, `2` for unknown peers, and `3` when access is unauthorized.
Results honour `peer_metrics_export` and `max_peer_metrics` limits, and can be
pushed to the `metrics-aggregator` for cluster-level views. See
[docs/operators/run_a_node.md](docs/operators/run_a_node.md) and
[docs/gossip.md](docs/gossip.md) for deeper usage.

All `net` commands bind to the loopback interface for safety.

Discovery, handshake, and proximity rules are detailed in [docs/localnet.md](docs/localnet.md).

Publish a DNS TXT record and query gateway policy:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":10,"method":"dns.publish_record","params":{"domain":"example.block","record":{"txt":"policy"},"sig":"<hex"}}'
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":11,"method":"gateway.dns_lookup","params":{"domain":"example.block"}}'
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":12,"method":"gateway.policy","params":{"domain":"example.block"}}'
```
`gateway.dns_lookup` reports whether a domain's public DNS record matches the on-chain entry. `gateway.policy` responses include `reads_total` and `last_access_ts` counters.
Domains outside `.block` must host a TXT record containing the on-chain public key to prevent spoofing. Operational details live in
[docs/gateway_dns.md](docs/gateway_dns.md).

## Telemetry & Metrics

An optional `metrics-aggregator` service collects peer statistics from multiple
nodes and exposes REST and Prometheus endpoints for fleet-wide monitoring. When
enabled via `metrics_aggregator.url` and `metrics_aggregator.auth_token` in
`config.toml`, nodes push snapshots that surface
`cluster_peer_active_total{node_id}` and `aggregator_ingest_total{node_id}`.
Secure deployments protect the channel with TLS and rotate the shared auth token
regularly. See [docs/monitoring.md](docs/monitoring.md) for deployment details
and alerting examples.

Quick start:

```bash
# launch the aggregator
metrics-aggregator --listen 127.0.0.1:9101 &

# point a node at it
AGGREGATOR_AUTH_TOKEN=secret \
blockctl node --config ~/.block/config.toml \
  --metrics-aggregator-url http://127.0.0.1:9101
```

The compute marketplace's cancellation API integrates with telemetry: calling
`compute.job_cancel` or `blockctl compute cancel` increments
`scheduler_cancel_total{reason}` and the node refunds any locked bonds.
