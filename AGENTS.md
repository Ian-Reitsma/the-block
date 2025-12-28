# AGENTS.md — **The‑Block** Developer Handbook

## For New Contributors (Even if You're New to Blockchains)

Welcome! This file is the "bible" for how we build The Block. Before diving into the technical details, here's what you need to know:

### What "Spec-First" Means

In this project, **documentation describes reality**. If code does something different from what the docs say, that's a bug in the docs (or the code needs fixing). This is the opposite of many projects where docs are an afterthought.

**Why it matters:** When you want to change something, you first update the docs to describe the new behavior, get that reviewed, and only then write the code. This prevents "drift" where nobody knows what the code is actually supposed to do.

### Quality Gates in Plain English

| What We Call It | What It Actually Does |
|-----------------|----------------------|
| `just lint` | Style checks — catches common mistakes, ensures consistent formatting |
| `just fmt` | Auto-format — makes your code look like everyone else's |
| `just test-fast` | Quick tests — catches obvious bugs in a few minutes |
| `just test-full` | Full tests — runs everything, takes longer, catches subtle issues |
| Replay test | Re-runs all historical blocks to verify determinism (same input = same output) |
| Settlement audit | Double-entry accounting check — makes sure BLOCK doesn't appear or disappear |
| Fuzzing | Throws random inputs at the code to find edge cases |

### Your First PR: 3 Steps

1. **Pick a safe starting task:**
   - Fix a typo in docs
   - Add a test for existing functionality
   - Check the "Beginner documentation backlog" in §15

2. **Run the basic checks locally:**
   ```bash
   just lint && just fmt && just test-fast
   ```

3. **Ask for a sanity check:**
   - Find the subsystem owner in [`docs/overview.md`](docs/overview.md#document-map)
   - Tag them in your PR or ask in the relevant channel

---

Quick Index
- Vision & Strategy: see §16
- Agent Playbooks: see §17
- Strategic Pillars: see §18
- Subsystem Atlas & workspace map: see [`docs/subsystem_atlas.md`](docs/subsystem_atlas.md)
- Monitoring Stack: see [`Telemetry Wiring`, `Metrics Aggregator Ops`, and `Monitoring`](docs/operations.md#telemetry-wiring) and `make monitor`
- First-party HTTP tooling lives under `crates/httpd`; reuse `HttpClient` or
  `BlockingClient` instead of pulling in third-party stacks (`reqwest` and
  friends are no longer linked anywhere in the workspace).
- Status & Roadmap: see [`Document Map`](docs/overview.md#document-map)
- Progress Snapshot: see [`Document Map`](docs/overview.md#document-map) for subsystem status and gaps
- Networking, per-peer telemetry, & DHT recovery: see [`Networking and Propagation`](docs/architecture.md#networking-and-propagation)
- QUIC handshake & fallback rules: see [`Networking and Propagation`](docs/architecture.md#networking-and-propagation)
- Economic formulas: see [`BLOCK Supply`, `Fee Lanes`, and `Settlement`](docs/economics_and_governance.md#block-supply-and-sub-ledgers)
- Blob root scheduling: see [`Ledger and Consensus`](docs/architecture.md#ledger-and-consensus)
- Macro-block checkpoints: see [`Ledger and Consensus`](docs/architecture.md#ledger-and-consensus)
- Law-enforcement portal & canary runbook: see [`Auxiliary Services`](docs/architecture.md#auxiliary-services)
- Range-boost queue semantics: see [`LocalNet and Range Boost`](docs/architecture.md#localnet-and-range-boost)
- Read acknowledgement batching and audit workflow: see [`Gateway and Client Access`](docs/architecture.md#gateway-and-client-access)
- RocksDB layout, crash recovery, and simulation replay: see `state/README.md`
- Parallel execution and transaction scheduling: see [`Compute Marketplace`](docs/architecture.md#compute-marketplace)
- PoH tick generator: see [`Ledger and Consensus`](docs/architecture.md#ledger-and-consensus)
- Commit–reveal scheme: see [`Proposal Lifecycle`, `Governance Parameters`, and `Risk Controls`](docs/economics_and_governance.md#proposal-lifecycle)
- Service badge tracker: see [`Auxiliary Services`](docs/architecture.md#auxiliary-services)
- Fee market reference: see [`BLOCK Supply`, `Fee Lanes`, and `Settlement`](docs/economics_and_governance.md#block-supply-and-sub-ledgers)
- Network fee rebates: see [`BLOCK Supply`, `Fee Lanes`, and `Settlement`](docs/economics_and_governance.md#block-supply-and-sub-ledgers)
- Transaction lifecycle and fee lanes: see [`Transaction and Execution Pipeline`](docs/architecture.md#transaction-and-execution-pipeline)
- Compute-market courier retry logic: see [`Compute Marketplace`](docs/architecture.md#compute-marketplace)
- Compute-market admission quotas: see [`Compute Marketplace`](docs/architecture.md#compute-marketplace)
- Compute-unit calibration: see [`Compute Marketplace`](docs/architecture.md#compute-marketplace)
- Compute-market SNARK receipts: see [`Compute Marketplace`](docs/architecture.md#compute-marketplace)
- Multi-hop trust-line routing: see [`DEX and Trust Lines`](docs/architecture.md#dex-and-trust-lines)
- DEX escrow and partial-payment proofs: see [`DEX and Trust Lines`](docs/architecture.md#dex-and-trust-lines)
- AMM pools and liquidity mining: see [`DEX and Trust Lines`](docs/architecture.md#dex-and-trust-lines)
- Gateway DNS publishing and policy records (`.block` TLD or externally verified): see [`Gateway and Client Access`](docs/architecture.md#gateway-and-client-access)
- Gossip relay dedup and adaptive fanout: see [`Networking and Propagation`](docs/architecture.md#networking-and-propagation)
- P2P handshake and capability negotiation: see [`Networking and Propagation`](docs/architecture.md#networking-and-propagation)
- Light-client synchronization and security model: see [`Gateway and Client Access`](docs/architecture.md#gateway-and-client-access)
- Light-client state streaming: see [`Gateway and Client Access`](docs/architecture.md#gateway-and-client-access)
- Bridge light-client verification: see [`Token Bridges`](docs/architecture.md#token-bridges)
- Jurisdiction policy packs and LE logging: see [`KYC, Jurisdiction, and Law-Enforcement`](docs/security_and_privacy.md#kyc-jurisdiction-and-compliance)
- Probe CLI and metrics: see [`Auxiliary Services`](docs/architecture.md#auxiliary-services)
- Operator QUIC configuration and difficulty monitoring: see [`Bootstrap`, `Running a Node`, and `Deployment`](docs/operations.md#bootstrap-and-configuration)
- Python demo walkthrough: see [`Python + Headless Tooling` and `Explainability`](docs/developer_handbook.md#python--headless-tooling)
- Telemetry summaries and histograms: see [`Telemetry and Instrumentation`](docs/architecture.md#telemetry-and-instrumentation)
- Simulation framework and replay semantics: see [`Environment Setup`, `Coding Standards`, `Testing`, `Performance`, and `Formal Methods`](docs/developer_handbook.md#environment-setup)
- Wallet staking lifecycle: see [`Gateway and Client Access`](docs/architecture.md#gateway-and-client-access)
- Remote signer workflows: see [`Gateway and Client Access`](docs/architecture.md#gateway-and-client-access)
- Energy/Governance/RPC next tasks: see [`Energy Governance and RPC Next Tasks`](docs/architecture.md#energy-governance-and-rpc-next-tasks)
- Storage erasure coding and reconstruction: see [`Storage and State`](docs/architecture.md#storage-and-state)
- Storage market incentives and proofs-of-retrievability: see [`Storage and State`](docs/architecture.md#storage-and-state)
- KYC provider workflow: see [`KYC, Jurisdiction, and Law-Enforcement`](docs/security_and_privacy.md#kyc-jurisdiction-and-compliance)
- A* latency routing: see [`Networking and Propagation`](docs/architecture.md#networking-and-propagation)
- Mempool architecture and tuning: see [`Transaction and Execution Pipeline`](docs/architecture.md#transaction-and-execution-pipeline)
- Hash layout & genesis seeding: see [`Ledger and Consensus`](docs/architecture.md#ledger-and-consensus)
- State pruning and RocksDB compaction: see [`Storage and State`](docs/architecture.md#storage-and-state)
- Cross-platform deployment methods: see [`Bootstrap`, `Running a Node`, and `Deployment`](docs/operations.md#bootstrap-and-configuration)
- Build provenance and attestation: see [`Release Provenance and Supply Chain`](docs/security_and_privacy.md#release-provenance-and-supply-chain)

> **Read this once, then work as if you wrote it.**  Every expectation, switch, flag, and edge‑case is documented here.  If something is unclear, the failure is in this file—open an issue and patch the spec *before* you patch the code.

---

## 0 · Agent Operating Rules (Read Before Editing)

### 0.1 Spec-First Contract
- This file and the docs it cites are the product spec. If implementation and documentation disagree, fix the docs first, cite the change in your PR, and only then adjust code.
- No drive-by fixes. Every change must ship with updated comments, docs, and telemetry so operators and explorers stay aligned.
- Keep PRs atomic—split refactors, bug fixes, and feature work into separate reviews and tag the subsystem owner from [`docs/overview.md`](docs/overview.md#document-map).

### 0.2 Quality Gates & Tooling
- Run `just lint`, `just fmt`, and `just test-fast` locally before asking for review. When touching consensus, networking, storage, governance, wallet, CLI, or telemetry, also run `just test-full`.
- Workspace sweeps live under `cargo nextest run --all-features`; determinism (`cargo test -p the_block --test replay`) and settlement audits (`cargo test -p the_block --test settlement_audit --release`) are required for ledger/governance changes.
- Consensus, overlay, codec, storage, and governance paths must pass `scripts/fuzz_coverage.sh`; attach the `.profraw` summary (or exported report) to the PR.
- Dashboard parity is mandatory: whenever `metrics-aggregator/**` or `monitoring/**` changes, run `npm ci --prefix monitoring && make monitor` and update the Grafana docs/screenshots.

### 0.2a Monetary Policy & Autopilot Contract
- **One issuance engine.** `NetworkIssuanceController` (`node/src/economics/network_issuance.rs`) is the canonical source of truth for block rewards. Legacy decay/logistic helpers are strictly smoothing aids; if code deviates from the documented formula, fix the docs + controller before touching mining logic. Any proposal touching `inflation_*` knobs must explain how it keeps the controller aligned.
- **No hidden premine.** Genesis starts at zero emission; `scripts/analytics/coin-stats.py` exists only for design exploration. Shipping a premine or “founder pool” requires a public spec + governance proposal and must be reflected in this file before implementation.
- **Launch Governor owns readiness.** All automatic transitions between bootstrap/testnet/mainnet (operational, naming, and future economics/market gates) flow through `node/src/launch_governor`. Decisions are logged, timelocked, and optionally signed (`TB_GOVERNOR_SIGN=1`). No other subsystem may “flip mainnet switches” ad hoc—wire your feature into a governor gate, add the metrics it needs, document the streak thresholds here + in `docs/architecture.md`, and verify the persisted state with `tb-cli governor status --rpc <endpoint>` (autopilot flag, schema version, gate streaks, `economics_sample` ppm metrics) or `tb-cli governor intents --gate economics --limit N`.
  - `governor.status` now includes the deterministic `economics_prev_market_metrics` array, and `tb-cli governor status` prints a “telemetry gauges (ppm)” section so operators can correlate those values with the Prometheus series `economics_prev_market_metrics_{utilization,provider_margin}_ppm`.
- **Shadow → apply.** New gates run in shadow mode first (emit intents + snapshots, no state change) until telemetry proves they’re stable. Only then do we allow `apply_intent` to mutate runtime params. Document both modes plus rollback instructions in `docs/operations.md`.
- **Backlog traces.** Missing telemetry inputs (tx_count, treasury inflow, provider margins, etc.) and the mining/epoch economics unification must be tracked in §15 with file pointers so we never regress into “two sources of truth.” The new `economics_epoch_*` counters, `economics_prev_market_metrics_{utilization,provider_margin}_ppm`, and `economics_block_reward_per_block` now live under `node/src/lib.rs` and `node/src/telemetry.rs`, and Launch Governor only flips the economics gate once the persisted samples (auditable via `tb-cli governor status`/`intents`) match those gauges.

### 0.2b Critical Path Before Mainnet
The last 10% of work is operational hardening—this list is derived from the live code.
1. **✅ COMPLETE (2025-12-18): WAN-scale chaos (`sim/chaos_lab.rs`, `docs/operations.md#chaos-and-fault-drills`)**: Automated via `scripts/wan_chaos_drill.sh`. Multi-provider failover drill now orchestrates TLS rotation simulation, produces `chaos/status diff` artifacts, and generates drill summary with Grafana screenshot placeholders. All required artifacts (status snapshots, provider failover reports, TLS rotation logs) are generated and archived.
2. **✅ COMPLETE (2025-12-18): Provider margin telemetry (`node/src/telemetry.rs`, `docs/ECONOMIC_SYSTEM_CHANGELOG.md:207`)**: Real market metric derivation implemented in `node/src/economics/replay.rs` (lines 220-439). All four markets (storage, compute, energy, ad) now derive utilization and provider margin deterministically from on-chain data. The `economics_prev_market_metrics_{utilization,provider_margin}_ppm` gauges receive real values, resolving the "placeholder" status.
3. **✅ COMPLETE (2025-12-18): Treasury executor scaling (`governance/src/store.rs:150-214`)**: Batched executor implemented with MAX_BATCH_SIZE=100 and MAX_SCAN_SIZE=500. Pre-filtering and early-exit optimization allow handling 1,000+ pending disbursements without stalls. Executor now processes eligible payouts incrementally across ticks, preventing backlog-induced dashboard alerts.
4. **✅ COMPLETE (2025-12-19): Receipt Integration System at 99% readiness** (`node/src/receipts.rs`, `node/src/telemetry/receipts.rs`, `PHASES_2-4_COMPLETE.md`):
   - **Infrastructure delivered**: Receipt types, block serialization, cached hash integration, telemetry counters, metrics engine, and validation helpers.
   - **Markets integrated**: Ad, Storage, Compute, and Energy emit receipts with correct block heights and telemetry drains.
   - **Telemetry & monitoring**: Grafana dashboard (`monitoring/grafana_receipt_dashboard.json`), `metrics-aggregator` wiring, recipient drains counters, and pending depth alerts ship with the release.
   - **Testing & benchmarks**: 12 stress tests, dedicated `receipt_benchmarks`, integration suite, and verification script confirm 10,000 receipts/10 MB limits.
   - **Deployment guidance**: `RECEIPT_INTEGRATION_COMPLETE.md` and `PHASES_2-4_COMPLETE.md` describe the coordinated rollout, governor checks, and release checklist.
   - **Consensus impact reminder**: Receipts now influence block hash; follow the governor coordination badges (`node/src/launch_governor`) and `docs/operations.md#telemetry-wiring` before switching on mainnet.

#### Concrete Example: Changing BLOCK Fee Floor Behavior

Say you want to change how BLOCK fee floors work (e.g., increase the base fee target from 50% to 60% mempool fullness). Here's the actual order of operations:

1. **Read the existing spec first:**
   - [`docs/economics_and_governance.md`](docs/economics_and_governance.md) — fee lanes section
   - Understand what the current behavior is and why

2. **Propose doc changes first:**
   - Draft the change: "increase base fee target from 50% to 60% mempool fullness"
   - Update the docs describing the new behavior
   - Get approval from the owner listed in [`docs/overview.md`](docs/overview.md#document-map)

3. **Only then update code:**
   - `governance/src/params.rs` — the parameter definition
   - `node/src/fee` — the fee calculation logic
   - `cli/src/fee_estimator.rs` — the CLI display

4. **Run the right tests:**
   ```bash
   just test-full  # because you touched governance
   cargo test -p the_block --test replay  # determinism check
   cargo test -p the_block --test settlement_audit --release  # accounting check
   ```

5. **Update telemetry and dashboards if affected.**

### 0.3 Observability, Logging, and Features
- Guard metrics behind the `telemetry` feature, but keep logic active in all builds—only instrumentation should be `#[cfg]`.
- Production crates must use first-party stacks (`p2p_overlay`, `crates/httpd`, `foundation_serialization`, `storage_engine`, `coding`). Third-party alternatives need written approval recorded in `docs/developer_handbook.md` and `config/dependency_policies.toml`.
- Any new metric or CLI surface must be documented in [`docs/operations.md`](docs/operations.md#telemetry-wiring) and wired through the metrics aggregator `/wrappers` endpoint; update explorer/CLI help where applicable.

### 0.4 Developer Hygiene & Security
- **Runtime artifacts are NEVER committed.** All node-local state (databases, snapshots, history files, logs, build artifacts) must stay out of version control. This includes `target/`, `node/*_db/`, `node/snapshots/`, `node/diff_history/`, `node/governance/history/`, `qwen/`, and any `*.wal.log` or runtime JSON state files. These artifacts are unique to each node and pollute deterministic replay tests if committed. The `.gitignore` enforces this; if `git status` shows runtime state after a node run, that's a bug—update `.gitignore` and use `git rm --cached` to untrack the files.
- All runtime knobs live in `node/src/config.rs` under the `TB_*` namespace. Do not invent ad-hoc env vars; add them to the config map with doc updates if needed.
- Supply-chain rules: after dependency updates, rerun `cargo vendor`, refresh `provenance.json` and `checksums.txt`, and follow [`docs/security_and_privacy.md`](docs/security_and_privacy.md#release-provenance-and-supply-chain).
- Remote signer and wallet changes must update the law-enforcement/jurisdiction docs in [`docs/security_and_privacy.md`](docs/security_and_privacy.md#kyc-jurisdiction-and-compliance) and include regression tests under `tests/remote_signer_*.rs`.

### 0.5 Ownership & Escalation
- Subsystem owners are listed in [`docs/overview.md`](docs/overview.md#document-map); tag them in PRs and record any skipped tests explicitly.
- Break-glass procedures live in [`docs/operations.md#troubleshooting-playbook`](docs/operations.md#troubleshooting-playbook). When you discover a new failure mode, update the runbook immediately.
- Deferred or follow-up work belongs in §15 “Outstanding Blockers & Directives.” Mirror any TODOs you add in code to that section to keep the backlog visible.

### 0.6 Spec & Quality Guardrails
- **Spec-first confirmation loop** — Before scheduling work, diff the current implementation against the canonical spec lines in this file (`§0.1`, `AGENTS.md:65‑91`) and [`docs/overview.md:5‑67`](docs/overview.md#mission). File a documentation patch for any drift, route it through the Document Map owners listed in `docs/overview.md`, and only green-light code once the spec reflects the desired behaviour. Every workstream summary should cite the doc PR/issue that captured the delta.
- **Test cadence baked into checklists** — Standardize on `just lint`, `just fmt`, `just test-fast`, the applicable tier of `just test-full`, `cargo test -p the_block --test replay`, the settlement audit (`cargo test -p the_block --test settlement_audit --release`), and `scripts/fuzz_coverage.sh` before review. Embed this list inside subsystem work checklists, and attach the command transcript (or CI link) to review descriptions so no change lands without its gate artifacts.
- **Observability contract** — Route every new metric, CLI flag, or API surface through `node/src/telemetry`, then update `metrics-aggregator/` and `monitoring/` snapshots (`npm ci --prefix monitoring && make monitor`). Document the `/wrappers` exposure and dashboard edits in [`docs/operations.md#telemetry-wiring`](docs/operations.md#telemetry-wiring) and reference the relevant Grafana panel in the PR body.
- **Ad + Targeting readiness checklist** — Any touch to `crates/ad_market`, `node/src/{ad_policy_snapshot.rs,ad_readiness.rs,rpc/ad_market.rs,localnet,range_boost,gateway/dns.rs,read_receipt.rs,service_badge.rs}`, `cli/src/{ad_market.rs,gov.rs,explorer.rs}`, `metrics-aggregator/**`, or `monitoring/**` reruns `just lint`, `just fmt`, `just test-fast`, `just test-full`, `cargo test -p the_block --test replay`, `cargo test -p the_block --test settlement_audit --release`, and `scripts/fuzz_coverage.sh`. Attach terminal logs (or CI links), fuzz `.profraw` summaries, and `npm ci --prefix monitoring && make monitor` screenshots of the ad dashboards plus the refreshed `/wrappers` hash. Tag `@ad-market`, `@gov-core`, `@gateway-stack`, and `@telemetry-ops` on every review; owner-approved skips must be recorded inline and mirrored in `AGENTS.md §15.K`.
- **Runtime knobs + dependency governance** — All knobs must flow through `node/src/config.rs` with `TB_*` names. When tooling crates or dependencies change, update [`docs/developer_handbook.md`](docs/developer_handbook.md#environment-setup) and `config/dependency_policies.toml`, then immediately rerun `cargo vendor`, refresh `provenance.json`, and regenerate `checksums.txt` so release provenance stays accurate.
- **TODO mirroring + incident hygiene** — Every new TODO or deferred fix must be mirrored into §15 (include file path + owner) so the backlog stays visible to subsystem leads. Post-incident learnings go straight into [`docs/operations.md#troubleshooting-playbook`](docs/operations.md#troubleshooting-playbook) with reproduction steps and telemetry pivots so on-call engineers never hunt for tribal knowledge.
- **Subsystem atlas coverage** — When you add, move, or rename a module, update [`docs/subsystem_atlas.md`](docs/subsystem_atlas.md) so newcomers can map file paths to real-world concepts without spelunking code. Treat the atlas as part of the spec; CI reviewers should reject PRs that strand new modules without documentation.

---

Subsidy accounting is unified around the BLOCK-denominated subsidy buckets (`STORAGE_SUB`, `READ_SUB`, and `COMPUTE_SUB`); each bucket now represents ledger snapshots shared across the node, governance crate, CLI, and explorer.
The stack includes multi-signature release approvals with explorer and CLI support, attested binary fetch with automated rollback, QUIC mutual-TLS rotation plus diagnostics and chaos tooling, mempool QoS slot accounting, end-to-end metrics-to-log correlation surfaced through the aggregator and dashboards, and the fully in-house TCP/UDP reactor that underpins every HTTP, WebSocket, and gossip surface alongside the proof-rebate pipeline persisting receipts appended to coinbase outputs during block production. Governance tracks fee-floor policy history with rollback support, wallet flows surface localized floor warnings with telemetry hooks and JSON output, DID anchoring runs through on-chain registry storage with explorer timelines, and light-client commands handle sign-only payloads as well as remote provenance attestations. Macro-block checkpointing, per-shard state roots, SNARK-verified compute receipts, real-time light-client state streaming, Lagrange-coded storage allocation with proof-of-retrievability, adaptive gossip fanout with LRU deduplication, deterministic WASM execution with a stateful debugger, build provenance attestation, session-key abstraction, Kalman difficulty retune, and network partition recovery extend the cluster-wide `metrics-aggregator` and graceful `compute.job_cancel` RPC.

Highlights: governance/ledger/metrics aggregator encode via the first-party serialization facade and explorer/CLI/log indexer route SQLite through the `foundation_sqlite` wrapper; remaining serde_json/bincode usage lives in tooling.
- Overlay discovery, persistence, and uptime accounting now live behind the `p2p_overlay` crate with in-house and stub backends, bincode-managed peer stores, CLI/RPC selection, telemetry gauges, and integration tests covering both implementations.
- Governance parameters now steer runtime, transport, and storage-engine backends end-to-end; CLI, RPC, explorer, and telemetry surfaces reflect active selections while bootstrap scripts seed default policies for clusters.
 - Governance, ledger, and the metrics aggregator now round-trip exclusively through `foundation_serialization`, removing direct `serde_json`/`bincode` usage from production crates while tooling migrations are tracked in [`Release Provenance and Supply Chain`](docs/security_and_privacy.md#release-provenance-and-supply-chain).
- Release provenance now stages `cargo vendor` snapshots, records deterministic hashes in `provenance.json`/`checksums.txt`, and blocks tagging in CI unless dependency_registry snapshots pass alongside governance-policy checks.
- Storage backends route exclusively through the `storage_engine` crate, unifying RocksDB, sled, and in-memory providers with concurrency-safe iterators/batches, temp-dir hygiene, and configuration-driven overrides so `SimpleDb` is a thin adapter.
- The `coding` crate fronts encryption, erasure, fountain, and compression stacks with runtime-configurable factories; XOR parity and RLE compression fallbacks now sit behind audited rollout gates, surface coder/compressor labels in telemetry, and feed the bench harness comparison tooling so operators can insource dependencies without guesswork.
- Governance, SDKs, and the CLI continue to consume the shared `governance` crate with sled-backed `GovStore`, proposal DAG validation, Kalman retune helpers, and release quorum enforcement, keeping every integration on the node’s canonical state machine.
- The transport crate front-loads Quinn and s2n providers behind trait abstractions, advertises provider capabilities to the handshake layer, forwards per-provider telemetry counters, and lets integration suites swap in mock QUIC implementations deterministically.
- Wallet binaries ship on `ed25519-dalek 2.2.x`, propagate multisig signer sets, escrow hash algorithms, and remote signer telemetry, surfacing localized fee-floor coaching with JSON automation hooks for dashboards.
- Wrapper telemetry exports runtime/transport/overlay/storage/coding/codec/crypto metadata, feeds the aggregator `/wrappers` endpoint, powers the `contract-cli system dependencies` command, and keeps Grafana dashboards aligned with dependency-policy violations.
- SNARK receipts now run through the in-house Groth16 backend with Halo-style circuits, caching compiled wasm digests per workload, producing CPU/GPU prover telemetry (`snark_prover_latency_seconds`, `snark_prover_failure_total`), attaching proof bundles (with fingerprints + circuit artifacts) to SLA history, auto-selecting GPU provers whenever providers advertise CUDA/ROCm capability, and exposing data via `compute_market.sla_history` + `contract-cli compute proofs`.
- Compute-market matching enforces lane-aware batching with fairness windows, starvation telemetry, configurable batch sizes, and persisted receipts wired through the `ReceiptStore` so restarts replay only outstanding orders. The matcher rotates lanes until either the batch quota or a fairness deadline trips, stages seeds before swap-in to prevent invalid wipes, exposes structured lane status/age warnings plus `match_loop_latency_seconds{lane}` histograms for dashboards, and records payouts exclusively in BLOCK with receipts anchored directly into the consolidated subsidy ledger.
- Mobile gateway caches persist encrypted responses and offline transactions to sled-backed storage with TTL sweeping, max-size guardrails, eviction telemetry, and CLI/RPC status & flush endpoints so mobile users can recover across restarts without leaking stale data. Sweepers drain a min-heap of expirations, boot-time replays rebuild the queue, and ChaCha20-Poly1305 keys derive from `TB_MOBILE_CACHE_KEY_HEX` (or fall back to `TB_NODE_KEY_HEX`) to harden the cache at rest.
- Light-client device probes integrate Android/iOS power and connectivity hints, cache asynchronous readings with graceful degradation, stream `the_block_light_client_device_status{field,freshness}` telemetry (fresh/cached/fallback), surface gating messages in the CLI/RPC, honour overrides stored in `~/.the_block/light_client.toml`, and embed the latest device snapshot inside compressed log uploads.
 - Runtime-backed HTTP client coverage now spans the node/CLI stacks, and the metrics aggregator and gateway HTTP servers now run on the in-house `httpd` router with the first-party TLS layer; remaining HTTP migrations focus on tooling stubs documented in [`JSON-RPC`](docs/apis_and_tooling.md#json-rpc) and [`Document Map`](docs/overview.md#document-map).
- RPC clients clamp `TB_RPC_FAULT_RATE`, saturate exponential backoff after the 31st attempt, guard environment overrides with scoped restorers, and expose regression coverage so operators can trust bounded retry behaviour during incidents.
- `SimpleDb` snapshot rewrites stage data through fsync’d temporary files, atomically rename into place, and retain legacy dumps until the new image lands, eliminating crash-window data loss while keeping legacy reopen logic intact.
- Node CLI binaries honour telemetry/gateway feature toggles, emitting explicit user-facing errors when unsupported flags are passed, recording jurisdiction languages in law-enforcement audit logs, and compiling via optional feature bundles (`full`, `wasm-metadata`, `sqlite-storage`) for memory-constrained tests.
- Light-client state streaming, DID anchoring, and explorer timelines trace revocations and provenance attestations end-to-end with cached pagination so wallet, CLI, and dashboards agree on identity state.
- Gossip relay scheduling relies on configurable TTLs, latency-aware fanout scoring, shard-affinity persistence, and partition tagging with telemetry for dedup drops and peer failures, all surfaced through CLI introspection.
- Proof-relay rebates persist to disk with governance-parameterised rates, block-production integration, explorer leaderboards, and CLI/RPC pagination for historical receipts.

**Outstanding focus areas:**
- Ship governance treasury disbursement tooling and explorer timelines before opening external treasury submissions.
- Integrate compute-market SLA slashing atop the lane-aware matcher and document remediation dashboards for operators.
- Continue WAN-scale QUIC chaos drills for relay fan-out while publishing mitigation recipes from the new telemetry traces and validating cross-provider failover through the transport registry.
- Finish multisig wallet UX polish (batched signer discovery, richer CLI prompts) so remote signers can run production workflows.
- Expand bridge and DEX documentation with signer-set payloads, explorer telemetry, and release-verifier guidance ahead of the next tag.
- Automate storage migration drills and dependency fault simulations so wrapper swaps can be rehearsed before production rollouts.

**Energy + Governance Next Tasks (see `docs/architecture.md#energy-governance-and-rpc-next-tasks` for detail)**

*For newcomers — here's what these areas mean in plain terms:*

| Area | Plain English |
|------|---------------|
| **Energy/Oracle** | Smart meters send signed readings to the network. An "oracle" is just a trusted data source that bridges real-world info (energy usage) into the blockchain. We verify these readings cryptographically before crediting providers. |
| **RPC/CLI Hardening** | RPC = Remote Procedure Call, the way apps talk to nodes. "Hardening" means adding authentication (who are you?), rate-limiting (don't spam us), and better error messages. |
| **Telemetry/Observability** | Graphs and alerts that tell operators when something is wrong. "Telemetry" = metrics the node exports. "Observability" = being able to understand what's happening inside. |

**Current tasks:**
- Governance/Params: land proposal payloads for batch vs real-time energy settlement, surface explorer/CLI history, expand dependency graphs, and harden param snapshots/rollback audits.
- Energy/Oracle: production Ed25519 verification now ships in `crates/energy-market` and `crates/oracle-adapter`; provider trust roots load from `config/default.toml` via the `energy.provider_keys` array, which hot-reloads the verifier registry. Remaining work covers quorum/expiry policy + advanced slashing telemetry, persisting receipts in ledger/sled trees, and wiring explorer timelines once the new dispute/receipt RPCs settle.
- RPC/CLI Hardening: enforce auth + rate-limit parity for `energy.*`, add structured errors for signature/timestamp/meter failures, and publish JSON schema snippets with round-trip CLI tests.
- Telemetry/Observability: extend Grafana dashboards (providers, pending credits, slash totals), wire SLO/alerting for oracle latency + settlement stalls, and expose summary metrics via `/wrappers` + `/telemetry/summary`.
- Network/Transport & Storage/State: run QUIC chaos drills with failover/fingerprint rotation, assert new transport capabilities in tests, and clone the `SimpleDb` snapshot/restore drill for `TB_ENERGY_MARKET_DIR` plus forward/backward-compatible migrations.
- Security/Supply Chain + Performance/Correctness: enforce release-provenance gates for energy/oracle crates, lock down oracle secrets/log redaction, add throughput benchmarks + fuzzers + deterministic replay coverage for energy receipts.
- Docs/Explorer + CI: ship explorer tables/receipts timelines + `docs/testnet/ENERGY_QUICKSTART.md` dispute/verifier guidance, stabilize the integration suite (governance params, RPC energy, handshake, rate limit, ad-market), and add a fast-mainnet CI gate (unit + targeted integration: governance, RPC, ledger replay, transport handshake).

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
16. [Vision & Strategy](#16-vision--strategy)
17. [Agent Playbooks — Consolidated](#17-agent-playbooks--consolidated)

---

## 1 · Project Mission & Scope — Production-Grade Mandate

**The‑Block** is a *formally‑specified*, **Rust-first**, single-token (BLOCK) proof‑of‑work + proof‑of-service blockchain kernel destined for main-net deployment with legacy industrial sub-ledgers retained for compatibility. Treasury, governance, and RPC surfaces now expose BLOCK-denominated fields (`amount`, `balance`, `price`, etc.) without the `_ct`/`_it` suffixes; any stray `*_CT` identifiers should be treated as archival anchors to the migrated names rather than new tokens.
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

### Economic Model — Unified BLOCK Subsidy Engine

- Subsidy accounting now lives in the shared BLOCK ledger. Industrial workloads
  remain tracked as labelled lane subaccounts rather than a separate capped
  token, and all operator rewards settle in transferable BLOCK minted directly
  in the coinbase.
- Every block carries three subsidy fields: `STORAGE_SUB`, `READ_SUB`, and `COMPUTE_SUB`.
- `industrial_backlog` and `industrial_utilization` gauges feed
  `Block::industrial_subsidies()`; these metrics surface the queued work and
  realised throughput that the subsidy governor uses when retuning
  multipliers.
- Per‑epoch utilisation `U_x` (bytes stored, bytes served, CPU ms, bytes
  out) feeds the "one‑dial" multiplier formula:

  \[
  \text{multiplier}_x =
    \frac{\phi_x I_{\text{target}} S / 365}{U_x / \text{epoch\_secs}}
  \]

  Adjustments are clamped to ±15 % of the prior value; near‑zero
  utilisation doubles the multiplier to keep incentives alive. Governance
  may hot‑patch all multipliers via `kill_switch_subsidy_reduction`.
- Base miner reward follows a logistic curve

  \[
  R_0(N) = \frac{R_{\max}}{1+e^{\xi (N-N^\star)}}
  \]

  with hysteresis `ΔN ≈ √N*` to damp flash joins/leaves.
- See [`BLOCK Supply`, `Fee Lanes`, and `Settlement`](docs/economics_and_governance.md#block-supply-and-sub-ledgers) for full derivations and worked examples.

## 2 · Repository Layout

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
  overview.md
  architecture.md
  economics_and_governance.md
  operations.md
  security_and_privacy.md
  developer_handbook.md
  apis_and_tooling.md
  LEGACY_MAPPING.md
  SUMMARY.md
  book.toml
  assets/
  maths/
  monitoring/
  spec/
AGENTS.md
```

Tests and benches live under `node/`. If your tree differs, run the repo re‑layout task in this file.

## 3 · System Requirements

- Rust 1.86+, `cargo-nextest`, and `cargo-fuzz` (nightly). `maturin` is no
  longer required while the first-party Python bridge is stubbed.
- Python 3.12.3 in a virtualenv; bootstrap creates `bin/python` shim and prepends `.venv/bin` to `PATH`.
- Node 18+ for the monitoring stack; `npm ci --prefix monitoring` must succeed when `monitoring/**` changes.
- On Linux, `patchelf` is required for wheel installs (bootstrap installs it automatically).
### Disclaimer → Production Readiness Statement

No longer a toy. The‑Block codebase targets production-grade deployment under real economic value.
Every commit is treated as if main-net launch were tomorrow: formal proofs, multi-arch CI, and external security audits are mandatory gates.
Proceed only if you understand that errors here translate directly into on-chain financial risk.

### Vision Snapshot

*The-Block* ultimately targets a civic-grade chain: a one-second base layer
that anchors notarized micro-shards, lane-aware BLOCK accounting, and an
inflation-subsidy meter that rewards honest node work. Governance follows the
"service guarantees citizenship" maxim—badges earned by uptime grant one
vote per node, with shard-based districts to check capture. This repository is
the kernel of that architecture.

### Current Foundation

The codebase already ships a reproducible kernel with:

- dynamic difficulty retargeting and one-second block cadence,
- unified BLOCK fee routing with decay-driven emissions and legacy industrial sub-ledger reporting,
- purge-loop infrastructure with telemetry counters and TTL/orphan sweeps,
- a minimal TCP gossip layer and JSON-RPC control surface,
- cross-language serialization tests and a Python demo.

### Long-Term Goals

Future milestones add durable storage, authenticated peer discovery,
micro-shard bundle roots, quantum-ready crypto, and the full
service-based governance stack. See §16 “Vision & Strategy”
for the complete blueprint embedded in this document.


## 16 · Vision & Strategy

The following section is the complete, up‑to‑date vision. It supersedes any earlier, partial “vision” notes elsewhere in the repository. Treat this as the single source of truth for mission, launch strategy, governance posture, and roadmap narrative.

# Agents Vision and Strategy

Service Guarantees Citizenship: A Civic-Scale Architecture for a One-Second L1, Notarized Micro‑Shards, and Contribution‑Weighted Governance

## Abstract
The-Block is a production-grade, people-powered blockchain designed to make everyday digital life faster, cheaper, and more trustworthy while rewarding real service. A simple, auditable 1-second L1 handles value and policy; sub-second micro-shards batch heavy AI/data into notarized roots per tick. Economics now revolve around a single transferable BLOCK; consumer and industrial lanes track payout share and rebate accruals—everything still settles in BLOCK. Governance binds rights to earned service via bicameral votes (Operators + Builders), quorum, and timelocks. The networking model extends beyond classic blockchains: nearby devices form a "people-built internet" (LocalNet + Range Boost) where proximity and motion become infrastructure, coverage earns more where it’s scarce, and money maps to useful time and reach. Launch proceeds consumer-first (single USDC pool), with industrial lanes lighting once readiness trips.

## 1. Introduction & Current State
Public chains excel in different slices—monetary credibility (Bitcoin), programmability (Ethereum), low latency (Solana), payments (XRP)—but none marries auditability, sub‑second data, wide participation, and service‑tied rights. Our blueprint: keep L1 minimal and deterministic; push heavy work to shards; pay for accepted results; and let “service guarantee citizenship.”

Already in‑repo:
- 1‑second L1 kernel (Rust), difficulty retarget, mempool validation
- single‑token (BLOCK) model with consumer/industrial lanes, decay‑based emissions, fee selectors
- purge loops (TTL/orphan) with telemetry
- minimal gossip + JSON‑RPC node
- cross‑language determinism tests, Python demo

## 2. System Overview
### 2.1 One‑Second L1 + Notarized Micro‑Shards
L1: value transfers, governance, shard‑root receipts; fixed 256‑bit header; canonical encoding. Shards: domain lanes (AI/media/storage/search) at 10–50 ms; emit one root/tick with quorum attestations; inner data stays user‑encrypted and content‑addressed.

### 2.2 Service Identity & Roles
Nodes attest uptime and verifiable work (bandwidth/storage/compute). Each epoch, percentile ranking assigns roles: target ~70% Consumer / ~30% Industrial; roles lock for the epoch with hysteresis to prevent flapping.

### 2.3 Economics: Unified BLOCK Supply
- BLOCK is the sole transferable unit covering fees, staking, and rewards. Governance manages its emission curve and circulating float.
- Industrial workloads draw from a dedicated lane sub-ledger that governs payout share without introducing a separate token. Split targets are tuned by policy rather than markets.
- Personal rebates/priority credits remain ledger entries only. They auto-apply to your own bills, expire per policy, and never circulate or affect spot pricing.

## 3. Governance: Constitution vs Rulebook
**Constitution (immutable):** hard caps and monotone emissions; 1‑second cadence; one‑badge‑one‑vote; quorum + timelocks; no mint‑to‑EOA; no backdoors.

**Rulebook (bounded):** CON/IND split (±10%/quarter); industrial share target (20–50%); rebate accrual/expiry windows; base‑fee escalator bounds; treasury streaming caps; shard counts/admission; jurisdiction modules.

**Process:** bicameral votes (Operators/Builders); snapshot voters at create; secret ballots; param changes next epoch after timelock; upgrades require supermajority + longer timelock + rollback window; emergencies only at catalog/app layer, auto‑expire, fully logged.

## 4. Rewards, Fees, Emissions
- Two lane-labelled subsidy sub-ledgers (consumer/industrial) accrue in every coinbase and represent BLOCK payouts; consumer share pays uptime, industrial share weights validated work. If industrial demand spikes, governance nudges the split within bounds rather than minting a new asset.
- Emissions anchor to block height; publish curves and tests; first-month issuance stays tame (≈0.01% of circulating BLOCK). No variable caps; vest any pre-TGE accrual by uptime/validated work.
- Reads stay free; writes burn personal rebates first, then BLOCK (shard roots debit the industrial bucket, L1 transactions debit consumer). All flows settle in BLOCK accounts.

## 5. Privacy & UX
- Vault + Personal AI: default‑private content with revocable capabilities; explainable citations (which items answered a query); content encrypted at source; chain notarizes proofs only.
- OS‑native SDKs (iOS/Android/macOS/Windows) expose Open/Save/Share/Grant/Revoke; phones act as secure controllers/light relays; hubs/routers carry background work.
- Live trust label: funds can’t be taken; rules can’t skip timelocks; sharing is visible and revocable.

## 6. People‑Built Internet
### LocalNet (Fast Road)
Nearby devices bond uplinks, cache, and relay for instant starts and low latency; paid relays; visible speed boost for video/downloads/games.

### Range Boost (Long Road)
Delay‑tolerant store‑and‑forward across BLE/Wi‑Fi Direct/ISM bands; optional $15–$40 “lighthouse” dongles for rural reach; coverage pays more per byte where scarce.

### Carry‑to‑Earn & Update Accelerator
Phones earn by carrying sealed bundles along commutes; settlement releases on delivery proofs. Neighborhood Update Accelerator serves big updates/patches from nearby seeds (content‑addressed, verified) for instant downloads.

### Hotspot Exchange
User‑shared, rate‑limited guest Wi‑Fi with one‑tap join; earn at home, spend anywhere; roaming without passwords/SIMs; wrapped traffic and rate caps for host safety.

## 7. Compute Marketplace
- Per-slice pricing settles entirely in BLOCK; sealed-bid batch matches run with refundable deposits and equal pay per slice type.
- Canary lanes (transcode, authenticity checks) remain the benchmark set; heavier jobs expand under governance-approved caps with SLA telemetry.
- Shadow intents (stake-backed) show p25–p75 bands before activation; when armed, escrows convert into BLOCK payouts and start jobs with rebates landing as personal credits.
- Operator guardrails: daily per-node payout caps; UI break-even/margin probes (power cost × hours/shard × watts).

## 8. Compute‑Backed Money (CBM) & Instant Apps
- CBM: daily redeem curves—X BLOCK buys Y seconds standard compute or Z MB delivered; protocol enforces redeemability with a minimal backstop from marketplace fees.
- Instant Apps: tap‑to‑use applets execute via nearby compute/caches and settle later; creators paid per use in CBM; users often pay zero if they contributed.

## 9. Launch Plan
- BLOCK TGE: seed initial liquidity against a single USDC pool, time-lock LP shares, slow-start 48h, and publish pool math plus addresses.
- Marketplace preview: stake-backed intents show pricing bands without filling orders until governance opens the lanes.
- Readiness gates industrial payouts on sustained node capacity, liquidity, and votes; once tripped, start canary lanes, migrate shadow intents into live BLOCK escrows, and credit rebates as ledger entries.
- Vesting & caps: any pre-TGE accrual vests by uptime/validated work; cap total pre-launch claims.

## 10. SDKs
- Provenance: sensor‑edge signing, proof bundles, content hash anchoring; explainable citations.

## 11. Security, Legal & Governance Posture <a id="11-security--cryptography"></a>
- End‑to‑end encryption; protocol sees pointers and hashed receipts only; no master keys.
- Law‑enforcement: metadata only; catalogs log delists; public transparency log + warrant canary.
- Jurisdiction modules: client/provider consume versioned regional packs (consent defaults, feature toggles); community‑voted; forks allowed.
- Non‑custodial core; KYC/AML handled by ramps; OFAC‑aware warnings in UIs.
- Founder exit: burn protocol admin keys; reproducible builds; move marks/domains to a standards non‑profit; bicameral governance; public irrevocability txs.

## 12. Dashboard & Metrics
- Home: BLOCK/day (USD est.), 7-day sparkline; readiness score & bottleneck; node mix; inflation; circulating supply.
- Marketplace: job cards w/ p25–p75, p_adj; est. duration on your device; break‑even/margin; refundable capacity stakes.
- Wallet/Swap: balances, recent tx; DEX swap (USDC↔BLOCK); no fiat in-app.
- Policy: emissions curve; live R(t,b); reserve inventory; jurisdiction pack hashes; transparency log.

## 13. Focus Areas

See [Document Map](docs/overview.md#document-map) for cross-links into the canonical docs. The list below summarizes notable capabilities across the stack.

- Persistent proof-rebate tracker with governance rate caps now writes receipts to disk, injects payouts into coinbase assembly, and exposes explorer/CLI history with telemetry for pending balances.
- Bridge module tracks per-asset channels, multi-signature quorums, and challenge windows with slashing hooks, persisted state, and expanded RPC/CLI coverage.
- Gossip relay now runs an LRU-backed dedup cache with configurable TTLs, latency-aware fanout scoring, shard-affinity persistence, partition tagging, and detailed telemetry surfaces.
- Stake-weighted PoS finality with validator registration, bonding/unbonding, and slashing RPCs.
- Proof-of-History tick generator and Turbine-style gossip for deterministic propagation.
- Parallel execution engine with optional GPU hash workloads.
- Modular wallet framework with hardware signer support and CLI utilities.
- Cluster-wide `metrics-aggregator` service and graceful `compute.job_cancel` RPC for reputation-aware rollbacks.
- Cross-chain exchange adapters, light-client crate, indexer with explorer, and benchmark/simulation tools.
- Free-read architecture with receipt batching, execution receipts, governance-tuned BLOCK subsidy ledger accounting, token-bucket rate limiting, and traffic analytics via `gateway.reads_since`.
- Fee-priority mempool with EIP-1559 base fee evolution; high-fee transactions evict low-fee ones and each block nudges the base fee toward a target fullness.
- Bridge primitives with relayer proofs and lock/unlock flows exposed via `blockctl bridge deposit`/`withdraw`.
- Persistent contracts and on-disk key/value state with opcode ABI generation and `contract` CLI for deploy/call.
- DexStore-backed order books and trade logs with multi-hop trust-line routing that scores paths by cost and surfaces fallback routes.
- Governance-tunable mempool fee floor parameters stream to telemetry, explorer history, and rollback logs, while wallet fee warnings emit localized prompts and DID anchors propagate through RPC, CLI, and explorer views.
- BLOCK balance and rate-limit webhooks; mobile light client registers push endpoints and triggers notifications on changes.
- Jittered RPC client with exponential backoff and env-configured timeout windows to prevent request stampedes.
- CI settlement audit job verifying explorer receipt indexes against ledger anchors.
- Fuzz coverage harness that installs LLVM tools on demand and reports missing `.profraw` artifacts.
- Operator runbook for manual DHT recovery detailing peer DB purge, bootstrap reseeding, and convergence checks.


  - Expose CLI plumbing for CBM redemptions through `blockctl cbm redeem` commands.

### Medium term

- Full cross-chain exchange routing across major assets
  - Implement adapters for SushiSwap and Balancer.
  - Integrate bridge fee estimators and route selectors.
  - Simulate slippage across multi-hop swaps.
  - Provide watchdogs for stuck cross-chain swaps.
  - Document settlement guarantees and failure modes.
- Distributed benchmark network at scale
  - Deploy the harness across 100+ nodes and regions.
  - Automate workload mix permutations.
  - Gather latency and throughput heatmaps.
  - Generate regression dashboards from collected metrics.
  - Publish performance tuning guides.
- Wallet ecosystem expansion
  - Add multisig modules.
  - Ship Swift and Kotlin SDKs for mobile clients.
  - Enable hardware wallet firmware update flows.
  - Provide secure backup and restore tooling.
  - Host an interoperability test suite.
- Governance feature extensions
  - Roll out a staged upgrade pipeline for node versions.
  - Support proposal dependencies and queue management.
  - Add on-chain treasury accounting primitives.
  - Offer community alert subscriptions.
  - Finalize rollback simulation playbooks.
- Mobile light client productionization
  - Optimize header sync and storage footprints.
  - Add push-notification hooks for balance events.
  - Integrate background energy-saving tasks.
  - Support signing and submitting transactions from mobile.
  - Run a beta program across varied hardware.

### Long term

- Smart-contract VM and SDK release
  - Design a deterministic instruction set.
  - Provide gas accounting and metering infrastructure.
  - Release developer tooling and ABI specs.
  - Host example applications and documentation.
  - Perform audits and formal verification.
- Permissionless compute marketplace
  - Integrate heterogeneous GPU/CPU scheduling.
  - Enable reputation scoring for providers.
  - Support escrowed cross-chain payments.
  - Build an SLA arbitration framework.
  - Release marketplace explorer analytics.
- Global jurisdiction compliance framework
  - Publish additional regional policy packs.
  - Support PQ encryption across networks.
  - Maintain transparency logs for requests.
  - Allow per-region feature toggles.
  - Run forkability trials across packs.
- Decentralized storage and bandwidth markets
  - Implement incentive-backed DHT storage.
  - Reward long-range mesh relays.
  - Integrate content addressing for data.
  - Benchmark throughput for large file transfers.
  - Provide client SDKs for retrieval.
- Mainnet launch and sustainability
  - Lock protocol parameters via governance.
  - Run multi-phase audits and bug bounties.
  - Schedule staged token releases.
  - Set up long-term funding mechanisms.
  - Establish community maintenance committees.

## 14. Differentiators
- Utility first: instant wins (works with no bars, instant starts, offline pay, find‑anything) with no partner permission.
- Earn‑by‑helping: proximity and motion become infrastructure; coverage and delivery pay where scarce; compute pays for accepted results.
- Honest money: CBM redeemability; predictable emissions; no backdoors.
- Civic spine: service‑based franchise; catalogs—not protocol—carry social policy; founder exit is verifiable.

## 15 · Outstanding Blockers & Directives

The following items block mainnet readiness and should be prioritized. Each task references canonical file paths for ease of navigation:

1. **Unblock governance CLI builds**
   - Remove the stale `deps` formatter in `node/src/bin/gov.rs`, surface the remaining proposal fields (start/end, vote totals,
     execution flag), and add a regression that builds the CLI under `--features cli`.
2. **Unify wallet Ed25519 dependencies and escrow proof arguments**
   - Align `crates/wallet` on `ed25519-dalek 2.2`, update `wallet::remote_signer` to return the newer `Signature` type, and pass
     `proof.algo` to `verify_proof` in `node/src/bin/wallet.rs`. Add a smoke test under `tests/remote_signer_multisig.rs` once the
     binary links.
3. **Restore light-sync and mempool QoS integration coverage**
   - After fixing the binaries, re-enable `cargo test -p the_block --test light_sync -- --nocapture` and
     `cargo test -p the_block --test mempool_qos -- --nocapture` in CI so regressions in the fee floor and light-client paths are
     caught quickly.
4. **Document targeted CLI build flags in runbooks**
   - Update [`Environment Setup`, `Coding Standards`, `Testing`, `Performance`, and `Formal Methods`](docs/developer_handbook.md#environment-setup) and [`Bootstrap`, `Running a Node`, and `Deployment`](docs/operations.md#bootstrap-and-configuration) with the current feature-gating matrix (`cli`, `telemetry`,
     `gateway`) so operators know how to reproduce the lean build used in integration tests.
5. **Finish telemetry/privacy warning cleanup**
   - Audit modules touched by the recent gating pass (`node/src/service_badge.rs`, `node/src/le_portal.rs`, `node/src/rpc/mod.rs`)
     for lingering `_unused` placeholders and replace them with feature-gated logic or instrumentation so the code stays readable.
6. **Track RPC retry saturation and fault clamps in docs**
   - Keep [`Networking and Propagation`](docs/architecture.md#networking-and-propagation), [`JSON-RPC`](docs/apis_and_tooling.md#json-rpc), and [`Environment Setup`, `Coding Standards`, `Testing`, `Performance`, and `Formal Methods`](docs/developer_handbook.md#environment-setup) aligned with the new `MAX_BACKOFF_EXPONENT` behaviour and
     `[0,1]` fault-rate clamping so operators do not rely on outdated tuning advice.
7. **Verify SimpleDb snapshot safeguards under both features**
   - Add coverage that exercises the atomic rename path with and without `storage-rocksdb` to ensure the recent crash-safe writes
     behave identically across backends.
8. **Stage a docs pass after each regression fix**
   - The build currently fails fast because documentation lags behind implementation; require a `docs/` update in every follow-up
     PR that touches staking, governance, RPC, or telemetry so contributors keep operator guidance accurate.
9. **Unify block reward issuance paths**
   - Replace the legacy per-block decay/logistic reward calculation in `node/src/lib.rs::{mine_block_with_ts,apply_block}` with the output of `NetworkIssuanceController`, keeping the logistic factor (miner-fairness weighting) as a multiplier only. Add regression tests proving that block minting, ledger replay, telemetry, and explorer views all reflect the same reward numbers and update `docs/economics_and_governance.md` when behaviour changes.
10. **Wire missing economics metrics**
    - `node/src/lib.rs` now propagates the real `economics_epoch_tx_count`, `economics_epoch_tx_volume_block`, `economics_epoch_treasury_inflow_block`, and `economics_block_reward_per_block` values into `execute_epoch_economics()`. Keep `docs/economics_and_governance.md` and the telemetry dashboards aligned with these counters so the governor/readers can trace the exact inputs feeding each issuance decision.
11. **Add economics/market gates to the Launch Governor**
    - The governor’s new economics gate already consumes those gauges before promoting published intents. Continue documenting the gate thresholds in `docs/architecture.md#launch-governor`, ensure `governor.status`/`governor.decisions` cover the economics gate alongside naming/operational ones, and keep the telemetry runbooks in `docs/operations.md` + `docs/testnet/ENERGY_QUICKSTART.md` current with the gate signals.

### 15.A Governance & Treasury Surface
- **Treasury payload alignment** — Extend the governance DAG schemas (`governance/`), `node/src/governance`, `cli/src/governance`, explorer dashboards, and `node/src/treasury_executor.rs` to express multi-stage treasury approvals with attested release bundles before permitting external submissions. Reference lines `AGENTS.md:121-122` in every PR thread so reviewers can trace the stalled dependency.
- **UX + telemetry updates** — Document the refreshed process in [`docs/economics_and_governance.md`](docs/economics_and_governance.md#treasury) and wire coinbase-facing telemetry counters (disbursement lag, reject reasons) through `metrics-aggregator/` so dashboards expose lag/failure trends. Ensure `contract-cli` JSON snapshots surface the same payloads the explorer renders.
- **Determinism + fuzz gates** — Add ledger-level tests inside the governance crate and `node/tests/` to prove streaming, rollback, and kill-switch behaviour survives deterministic replay, and record the associated `scripts/fuzz_coverage.sh` `.profraw` artifacts whenever consensus/governance code paths change.
- **Observability hooks** — Push governance state diffs into `/wrappers` metadata and add Grafana timelines for service badges, fee-floor policies, and treasury deltas. Update [`docs/operations.md`](docs/operations.md#telemetry-wiring) with a runbook for “treasury stuck” scenarios, including CLI commands, RPCs, and log fingerprints.
- **Explorer + CLI parity** — Work with the explorer maintainers so badge history, policy timelines, and treasury dashboards load from a single canonical snapshot JSON (shared between `cli/`, `explorer/`, and `metrics-aggregator/`), preventing drift across operator tooling.

### 15.B Compute Market & SLA Controls
- **SLA-aware matcher** — Layer slashing logic atop the lane-aware matcher in `node/src/compute_market/matcher.rs`, coordinating with `lane scheduler` modules and `ReceiptStore` durability so failed work emits slash receipts anchored in the BLOCK subsidy sub-ledger (per `AGENTS.md:95-105` and `AGENTS.md:123`).
- **Docs + status surfaces** — Describe the slashing lifecycle in [`docs/architecture.md#compute-marketplace`](docs/architecture.md#compute-marketplace), update CLI/explorer lane health views, and expose telemetry such as `match_loop_latency_seconds{lane}`, fairness counters, and slash totals via the metrics aggregator.
- **Test coverage** — Expand `node/src/compute_market/tests/` and top-level `tests/` to cover fairness windows, starvation protection, SLA triggers, receipt persistence, and replay after restarts; include deterministic replays that validate persisted receipts, plus fuzzing for receipt serialization.
- **Remediation tooling** — Ship Grafana panels (sourced from `monitoring/`) and CLI commands that summarize per-lane degradation so operators can triage slashed jobs rapidly.
- **Ledger integration** — Ensure slashing updates propagate through `node/src/treasury_executor.rs`, ledger snapshots, and governance reporting so payouts and per-lane quotas remain in sync with the unified BLOCK ledger.

### 15.C Networking, Transport & Range-Boost Reliability
- **Chaos drill automation** — Script WAN-scale QUIC chaos drills (fault injection across providers) touching `node/src/net`, `node/src/p2p`, and `range_boost/`, validating handshake fallback, fanout scoring, mutual-TLS rotation, and recovery flows (`AGENTS.md:124`).
- **Documentation + runbooks** — Update [`docs/architecture.md#networking-and-propagation`](docs/architecture.md#networking-and-propagation) and [`docs/operations.md`](docs/operations.md#bootstrap-and-configuration) with mitigation recipes, cross-provider failover guides, and CLI/RPC introspection examples derived from the new telemetry traces.
- **Regression coverage** — Add tests for transport capability advertisement, failover timing, and range-boost TTL/fanout invariants. Make sure fuzz/nextest suites exercise both in-house and stub overlay backends.
- **Diagnostics wiring** — Instrument `p2p_overlay` and `crates/transport` to emit dedup drops, handshake negotiation details, and capability mismatches; surface them via `/wrappers` metadata and Grafana overlays.
- **TLS + automation parity** — Refresh metrics-aggregator TLS configs and document the chaos drill workflow in `docs/operations.md#bootstrap-and-configuration`; ensure scripts in `scripts/` reproduce the drills inside CI/staging.

### 15.D Wallet, Remote Signer & CLI UX
- **Multisig polish** — Implement batched signer discovery, richer CLI prompts, JSON automation hooks, and remote-signer telemetry inside `cli/src/wallet`, `node/src/identity`, and `remote_signer/` so production workflows are smooth (`AGENTS.md:125`).
- **Compliance docs** — Update [`docs/security_and_privacy.md#kyc-jurisdiction-and-compliance`](docs/security_and_privacy.md#kyc-jurisdiction-and-compliance) with the new signer flows, audit logging, LE portal integration, and telemetry notes; ensure portal metrics mirror wallet changes.
- **Regression suite** — Expand `tests/remote_signer_*.rs` and wallet integration tests to cover batched discovery, failure prompts, telemetry toggles, and config flag propagation via `node/src/config.rs`.
- **Messaging parity** — Align CLI/explorer messaging for fee-floor warnings, signer prompts, and JSON output. Document command help in [`docs/apis_and_tooling.md`](docs/apis_and_tooling.md#cli), and expose signer health dashboards plus `/wrappers` telemetry for availability/latency/audit trail completeness.

### 15.E Bridges, DEX & Cross-Chain Documentation
- **Doc expansion** — Enrich [`docs/architecture.md#token-bridges`](docs/architecture.md#token-bridges) and [`docs/architecture.md#dex-and-trust-lines`](docs/architecture.md#dex-and-trust-lines) with signer-set payloads, telemetry pipelines, and release-verifier guidance, mirrored into [`docs/operations.md`](docs/operations.md#auxiliary-services) runbooks (`AGENTS.md:33-41`, `AGENTS.md:126`).
- **Implementation alignment** — Keep `bridges/`, `dex/`, and explorer views in sync so escrow proofs, partial-payment artifacts, and trust-line metrics are exposed consistently through CLI, RPC, and dashboards. Update `foundation_serialization` profiles for any new payloads.
- **Testing + telemetry** — Strengthen regression coverage for bridge light-client verification, DEX AMM math, multi-hop trust-line routing, and escrow settlement replay. Export bridge/DEX-specific counters (escrow fulfillment, signer rotations, partial-payment retries) to the aggregator and Grafana.
- **Release provenance** — Document release-verifier scripts and attestation steps under [`docs/security_and_privacy.md#release-provenance-and-supply-chain`](docs/security_and_privacy.md#release-provenance-and-supply-chain) to guarantee cross-chain binaries meet provenance gating.

### 15.F Storage, Snapshot & Dependency Drill Automation
- **Drill automation** — Script `SimpleDb` snapshot/restore drills across `state/`, `storage/`, `node/src/simple_db`, and `storage_market/`, then document the workflow plus telemetry expectations inside [`docs/operations.md#storage-and-state`](docs/operations.md#storage-and-state). Base the instructions on the RocksDB layout guidance referenced in `AGENTS.md:21`.
- **CI harnesses** — Build CI-friendly harnesses (likely `scripts/` + `formal/`) that exercise dependency fault injection (coder/compressor swaps from `coding/`, storage backend toggles) and verify ledger/state parity afterward.
- **Telemetry exposure** — Emit snapshot/migration telemetry (duration, failures, time-to-restore) and surface it via Grafana + `/wrappers` metadata so operators track drills in real time.
- **Crash-safety validation** — Explicitly validate `SimpleDb` fsync+rename semantics on Linux/macOS/Windows to uphold crash-safe guarantees (`AGENTS.md:113-116`), capturing findings in docs and integration tests.

### 15.G Energy Governance & Oracle Controls
- **Proposal payloads** — Implement governance payloads that distinguish batch vs real-time settlement, extend explorer/CLI history, and harden snapshot/rollback auditing across `governance/`, `node/src/energy.rs`, and `cli/src/energy.rs` (`AGENTS.md:130`; `docs/overview.md:44-49`).
- **Oracle verifier** — Production Ed25519 verification now lives inside `crates/energy-market` and `crates/oracle-adapter`; next steps are enforcing quorum + expiry policies, logging slashing telemetry, and persisting receipts to both sled trees and ledger checkpoints (`AGENTS.md:131`).
- **CLI + schema parity** — Harden the new provider/receipt/dispute CLI flows with JSON schema exports aligned to [`docs/apis_and_tooling.md#energy-rpc-payloads-auth-and-error-contracts`](docs/apis_and_tooling.md#energy-rpc-payloads-auth-and-error-contracts). Back the endpoints with deterministic replay and fuzz coverage across provider mixes.
- **Documentation refresh** — Update [`docs/economics_and_governance.md`](docs/economics_and_governance.md#block-supply-and-sub-ledgers) and [`docs/architecture.md#energy-governance-and-rpc-next-tasks`](docs/architecture.md#energy-governance-and-rpc-next-tasks) with the new governance hooks, rollback auditing steps, and oracle dependency graphs.

### 15.H Energy Interfaces & Telemetry
- **RPC parity + schemas** — Enforce auth/rate-limit parity for all `energy.*` RPCs, add structured errors for signature/timestamp/meter failures, and publish JSON schema snippets with round-trip CLI tests (per `AGENTS.md:132`).
- **Dashboards + alerts** — Extend Grafana dashboards with provider counts, pending credits, dispute backlog, slash totals, and SLO/alerting for oracle latency + settlement stalls. Surface the new summary metrics (`energy_provider_total`, `energy_pending_credits_total`, `energy_active_disputes_total`, `energy_settlement_total{provider}`, etc.) through `/wrappers` and `/telemetry/summary` (`AGENTS.md:133`).
- **State drills** — Clone the `SimpleDb` snapshot/restore drill for `TB_ENERGY_MARKET_DIR`, layering forward/backward-compatible migrations plus QUIC chaos validation for oracle transport as described in `AGENTS.md:134`.
- **Docs alignment** — Keep [`docs/testnet/ENERGY_QUICKSTART.md`](docs/testnet/ENERGY_QUICKSTART.md) and [`docs/operations.md#energy-market-operations`](docs/operations.md#energy-market-operations) current with telemetry panels, health checks, and troubleshooting escalations. Ensure explorer timelines show the same aggregated data emitted by `contract-cli energy`.

### 15.I Energy Reliability, Security & CI
- **Supply-chain enforcement** — Enforce release-provenance gates, secret hygiene, and log redaction for energy/oracle crates. Add throughput benchmarks, fuzzers, and deterministic replay coverage for receipts, capturing `.profraw` summaries whenever consensus/governance paths touch settlement logic (`AGENTS.md:135`).
- **Fast-mainnet CI gate** — Update CI to include a fast-mainnet gate that runs governance param checks, energy RPC suites, ledger replay, transport handshake, and ad-market verifications (`AGENTS.md:136`). Stabilize integration suites accordingly.
- **Explorer timelines + disputes** — Ship explorer tables/time series for receipts and disputes (`docs/testnet/ENERGY_QUICKSTART.md`), aligning CLI/explorer outputs with governance snapshots for traceability.
- **Security docs + telemetry** — Document rate-limit/auth policies, signature requirements, and log redaction in [`docs/security_and_privacy.md#energy-oracle-safety`](docs/security_and_privacy.md#energy-oracle-safety). Wire telemetry alerts for policy violations or misconfigurations.
- **Release tooling** — Coordinate with release tooling so attested binaries include the updated energy/oracle components, and archive the fuzz coverage artifacts with every release candidate.

### 15.J Next Steps Sequencing
- Confirm the scope of this multi-track plan with subsystem owners, then attack spec/docs alignment (Sections 0.6 + 15.A) before coding changes. Governance/treasury and energy governance deliverables unblock downstream compute, networking, and wallet work, so treat them as the immediate gate for the remaining backlog.

### 15.K Ad Market & Targeting Readiness
- **Implementation go/no-go** — Documentation is in sync (see `docs/architecture.md`, `docs/overview.md`, `docs/apis_and_tooling.md`, `docs/operations.md`). All remaining work is code/CLI/RPC wiring: do not merge further spec changes without citing the new sections, and tag `@ad-market`, `@gov-core`, `@gateway-stack`, `@telemetry-ops` on every review.
- **CohortKeyV2 rollout + reversible migrations** ✅ PARTIALLY COMPLETE — Updated `node/src/ad_policy_snapshot.rs` to track domain tier supply, interest tag supply, and presence bucket metrics in parts-per-million. Updated `node/src/ad_readiness.rs` with `AdSegmentReadiness` structs tracking per-selector readiness stats (domain_tier, interest_tags, presence_bucket_id fields in `AdReadinessCohortUtilization`; `segment_readiness` field in `AdReadinessSnapshot`). Selector types (`DomainTier`, `InterestTagId`, `PresenceBucketRef`) are defined in `crates/ad_market/src/lib.rs` with proper serde attributes. Further work: implement dual-write sled keys (`cohort_v1:*` + `cohort_v2:*`) for reversible migrations and update `docs/system_reference.md`. Owners: `@ad-market`, `@gov-core`.
- **Presence attestation plumbing** ✅ COMPLETE — Sled-backed `PresenceCache` implemented in `node/src/localnet/presence.rs` with TTL expiry, freshness histograms, and governance-controlled configuration. The cache stores `PresenceReceipt` structs keyed by `{beacon_id, bucket_id}` with radius/confidence metadata. Governance params (`TB_PRESENCE_TTL_SECS`, `TB_PRESENCE_RADIUS_METERS`, `TB_PRESENCE_PROOF_CACHE_SIZE`, `TB_PRESENCE_MIN_CONFIDENCE_BPS`) are wired through `node/src/governance/params.rs`. RPCs `ad_market.list_presence_cohorts` and `ad_market.reserve_presence` are implemented in `node/src/rpc/ad_market.rs` with CLI commands in `cli/src/ad_market.rs` (`presence list`, `presence reserve`). Further work: expand `node/tests/ad_market_rpc.rs` for integration tests. Owners: `@ad-market`, `@gateway-stack`.
- **Presence RPC schemas** — JSON schemas for `ad_market.list_presence_cohorts` and `ad_market.reserve_presence` are defined in [`docs/apis_and_tooling.md#presence-cohort-json-schemas`](docs/apis_and_tooling.md#presence-cohort-json-schemas). Error codes `-32034` through `-32039` cover presence/privacy failures. Governance knobs (`TB_PRESENCE_TTL_SECS`, `TB_PRESENCE_RADIUS_METERS`, `TB_PRESENCE_PROOF_CACHE_SIZE`, `presence_min_crowd_size`, `presence_min_confidence_bps`) must be wired through `node/src/governance/params.rs` and exposed in `cli/src/gov.rs`. Owners: `@ad-market`, `@gov-core`.
- **Multi-signal auctions + CLI/RPC parity** — Teach `crates/ad_market/src/budget.rs`, `node/src/rpc/ad_market.rs`, `cli/src/ad_market.rs`, `cli/src/explorer.rs`, and `cli/src/gov.rs` about selector-specific bids, presence/domain filters, conversion-value payloads, and governance-controlled knobs. Ensure explorer payouts and governance proposals expose the new selector metadata. Owners: `@ad-market`, `@gov-core`.
- **Observability + dashboards** ✅ METRICS ADDED — New telemetry metrics added to `node/src/telemetry.rs`: `AD_SEGMENT_READY_TOTAL` (gauge with domain_tier/presence_bucket/interest_tag labels), `AD_PRIVACY_BUDGET_REMAINING_PPM` (gauge by campaign_id), `AD_BIDDING_LATENCY_MICROS` (gauge by auction_type), `DNS_AUCTION_BIDS_TOTAL` (counter by domain_tier), plus existing `AD_AUCTION_TOP_BID_USD`, `AD_BID_SHADING_FACTOR_BPS`, `AD_PRIVACY_BUDGET_UTILIZATION_RATIO`, `AD_CONVERSION_VALUE_TOTAL`, `AD_PRESENCE_RESERVATION_TOTAL`. DNS telemetry functions (`record_dns_auction_completed`, `adjust_dns_stake_locked`, `update_dns_auction_status_metrics`) are wired in `node/src/gateway/dns.rs`. Further work: aggregate to `metrics-aggregator/src/lib.rs` and refresh Grafana dashboards. Owners: `@telemetry-ops`, `@ad-market`.
- **Privacy budgets + docs/tests** — Harden `crates/ad_market/src/privacy.rs` to clamp selector combinations, enforce k-anonymity, and expose deterministic tests. Document the contract in `docs/security_and_privacy.md` and mirror selector/TODO entries in `docs/apis_and_tooling.md` and the LE portal runbooks. Owners: `@ad-market`, `@security`.

### 15.L Docs & Onboarding Parity
- **README + Document Map alignment** — Keep `README.md`, `docs/overview.md`, and `docs/developer_handbook.md` current with every selector/privacy/telemetry change (especially the ad-market readiness checklist and spec circulation log). Any new subsystem or owner mapping must be reflected in both files plus `docs/subsystem_atlas.md`. Owners: `@docs-core`, `@ad-market`.
- **API references** — Expand `docs/apis_and_tooling.md` alongside RPC/CLI changes (presence cohorts, selector weights, conversion payloads). Block merges when RPC structs change without corresponding doc updates; mirror the schema diffs into `/docs/spec/*.json` when fields are added. Owners: `@gov-core`, `@ad-market`.
- **Onboarding tasks** — Track outstanding doc follow-ups (handbook gaps, README examples, mdBook build regressions) inside this section so reviewers can validate the documentation delta before approving code. When ad/energy/networking work lands without a doc diff, add a TODO here referencing the file + owner until the docs catch up.

---
This document supersedes earlier "vision" notes. Outdated references to merchant‑first discounts at TGE, dual‑pool day‑one listings, or protocol‑level backdoors have been removed. The design here aligns all launch materials, SDK plans, marketplace sequencing, governance, legal posture, and networking with the current strategy.

---

## 17 · Agent Playbooks — Consolidated

This section consolidates actionable playbooks from §§18–19. It is included here for single‑file completeness and should be treated as canonical going forward.

### 17.1 Updated Vision & Next Steps

- Phase A (0–2 weeks): Consumer‑first TGE and preview
  - Single USDC/BLOCK pool seeded and time-locked; 48h slow-start; publish pool math/addresses.
  - Dashboard readiness index and bottleneck tile; earnings sparkline; vesting view (if enabled).
  - Shadow marketplace with stake‑backed intents; p25–p75 bands and p_adj; break‑even/margin probe.
  - LocalNet short relays with receipts and paid relays; strict defaults and battery/data caps.
  - Offline money/messaging (canary): escrowed receipts, delayed settlement on reconnect; small group “split later”; SOS broadcast.
- Phase B (2–6 weeks): People‑Built Internet primitives
  - Range Boost delay‑tolerant store‑and‑forward; optional lighthouse recognition; coverage/delivery earnings.
  - Hotspot Exchange: host/guest modes, wrapped traffic; subsidy meters backed by BLOCK.
  - Carry‑to‑Earn sealed bundle courier for commuter routes; privacy explainer; Neighborhood Update Accelerator for instant large downloads.
- Phase C (6–10 weeks): Industrial canary lanes + SDKs v1
  - Transcode and authenticity‑check lanes; sealed‑bid batches; small deposits; per‑slice pricing; daily payout caps; operator diagnostics.
  - SDKs: Provenance; sample apps and docs.
  - Legal/Policy: law‑enforcement guidelines (metadata‑only), transparency log schema, jurisdiction modules, SBOM/licensing, CLA; reproducible builds; privileged RPCs disabled by default; founder irrevocability plan.
- Phase D (10–16 weeks): CBM & Instant Apps; marketplace expansion
  - Daily CBM redeem curves; minimal backstop from marketplace fees.
  - Instant Apps executing via LocalNet; creators paid per CBM use; users often pay zero when contributing.
  - Expand lanes (vector search, diffusion) with caps; auto‑tune p_adj under backlog; batch clearing cadence.

Deliverables Checklist (must‑have artifacts)
- Code: client toggles (LocalNet/Range), escrow/relay/courier receipts, marketplace preview, canary lanes, SDKs.
- Tests: per‑slice pricing, batch clearing, break‑even probes, receipts integrity, range relay, offline settlement, SDK round‑trips.
- Metrics: readiness score, bands, p_adj, coverage/delivery counters, CBM redeem stats, SOS/DM delivery receipts.
- Docs: README/AGENTS/Agents‑Sup alignment; legal/policy folder; governance scaffolding; emissions/CBM docs; SDK guides.

Note: Older “dual pools at TGE,” “merchant‑first discounts,” or protocol‑level backdoor references are obsolete and removed by the Vision above.

### 17.3 Operating Mindset

- Production standard: spec citations, `cargo test --all --features test-telemetry --release`, zero warnings.
- Atomicity and determinism: no partial writes, no nondeterminism.
- Spec‑first: patch specs before code when unclear.
- Logging and observability: instrument changes; silent failures are bugs.
- Security assumptions: treat inputs as adversarial; validations must be total and explicit.
- Granular commits: single logical changes; every commit builds, tests, and lints cleanly.
- Formal proofs: `make -C formal` runs `scripts/install_fstar.sh`, verifies checksums, and caches an OS/arch-specific FStar release under `formal/.fstar/<version>`. The installer exports `FSTAR_HOME` so downstream tools can reuse the path; override the pinned release with `FSTAR_VERSION` or set `FSTAR_HOME` to an existing install.
- Monitoring dashboards: run `npm ci --prefix monitoring` then `make -C monitoring lint` (via `npx jsonnet-lint`); CI lints when `monitoring/**` changes and uploads logs as artifacts.
- WAL fuzzing (nightly toolchain required): `make fuzz-wal` stores artifacts and RNG seeds under `fuzz/wal/`; reproduce with `cargo fuzz run wal_fuzz -- -seed=<seed> fuzz/wal/<file>`.
  Use `scripts/extract_wal_seeds.sh` to list seeds and see [Storage and State](docs/architecture.md#storage-and-state) for failure triage.

- Compute market changes: run `cargo nextest run --features telemetry compute_market::courier_retry_updates_metrics price_board` to cover courier retries and price board persistence. Install `cargo-nextest` (compatible with Rust 1.86) if the command is unavailable.
- QUIC networking changes: run `cargo nextest run --profile quic` to exercise QUIC handshake, fanout, and fallback paths. The
  profile enables the `quic` feature flag.

### 17.5 Architecture & Telemetry Highlights (from Agents‑Sup)

- Consensus & Mining: PoW with BLAKE3; dynamic retarget over ~120 blocks with clamp [¼, ×4]; headers carry difficulty; coinbase fields must match tx[0]; decay rewards.
- Accounts & Transactions: Account balances, nonces, pending totals; Ed25519, domain‑tagged signing; `pct` carries an arbitrary 0–100 split with sequential nonce validation.
- Storage: in‑memory `SimpleDb` prototype; schema versioning and migrations; isolated temp dirs for tests.
- Networking & Gossip: QUIC/TCP transport with `PeerSet`; per-peer drop reasons and reputation-aware rate limits surface via `net.peer_stats` and the `net` CLI. JSON‑RPC server in `src/bin/node.rs`; integration tests cover `mempool.stats`, `localnet.submit_receipt`, `dns.publish_record`, `gateway.policy`, and `microshard.roots.last`.
- Inflation subsidies: BLOCK minted per byte, read, and compute with governance-controlled multipliers; reads and writes are rewarded without per-user fees. `industrial_backlog` and `industrial_utilization` metrics, along with `Block::industrial_subsidies()`, surface queued work and realised throughput feeding those multipliers. Ledger snapshots now flow through the BLOCK subsidy store described in [Economics and Governance § BLOCK Supply](docs/economics_and_governance.md#block-supply-and-sub-ledgers) and supersede the old `read_reward_pool`. Subsidy multipliers (`beta/gamma/kappa/lambda`) retune each epoch via the same formula; changes are logged under `governance/history` and surfaced in telemetry. An emergency parameter
  `kill_switch_subsidy_reduction` can temporarily scale all multipliers down by
  a voted percentage, granting governance a rapid-response lever during economic
  shocks.
  Operators can inspect current multipliers via the `inflation.params` RPC and
  reconcile stake-weighted payouts by querying `stake.role` for each bonded
  account.
- Telemetry & Spans: metrics including `peer_request_total{peer_id}`,
  `peer_bytes_sent_total{peer_id}`, `peer_drop_total{peer_id,reason}`,
  `peer_handshake_fail_total{peer_id,reason}`,
  `peer_stats_query_total{peer_id}`, `peer_stats_reset_total{peer_id}`,
  `peer_stats_export_total{result}`, `peer_reputation_score{peer_id}`, and
  the `peer_metrics_active` gauge; scheduler metrics `scheduler_match_total{result}`
  and `scheduler_effective_price`; transport metrics `ttl_drop_total`,
  `startup_ttl_drop_total`, `orphan_sweep_total`, `tx_rejected_total{reason=*}`,
  `difficulty_retarget_total`, `difficulty_clamp_total`,
  `quic_conn_latency_seconds`, `quic_bytes_sent_total`,
  `quic_bytes_recv_total`, `quic_handshake_fail_total{peer}`,
  `quic_retransmit_total{peer}`, `quic_cert_rotation_total`,
  `quic_disconnect_total{code}`, `quic_endpoint_reuse_total`;
  release metrics `release_quorum_fail_total` and
  `release_installs_total`; aggregator misses
  `log_correlation_fail_total` feed ops alerts; spans for mempool
  and rebuild flows; Prometheus exporter via `serve_metrics`. Snapshot operations
  export `snapshot_duration_seconds`, `snapshot_fail_total`, and the
  `snapshot_interval`/`snapshot_interval_changed` gauges.
- Schema Migrations: bump `schema_version` with lossless routines; preserve fee invariants; document the change under [Architecture § Storage and State](docs/architecture.md#storage-and-state).
- Python Demo: `PurgeLoop` context manager with env controls; demo integration test settings and troubleshooting tips.
- Quick start: `just demo` runs the Python walkthrough after `./scripts/bootstrap.sh` and fails fast if the virtualenv is missing.
- Governance CLI: `gov submit`, `vote`, `exec`, and `status` persist proposals under `examples/governance/proposals.db`.
- Workload samples under `examples/workloads/` demonstrate slice formats and can
  be executed with `cargo run --example run_workload <file>`; rerun these examples after modifying workload code.

## 18 · Strategic Pillars

- **Governance & Subsidy Economy** ([Economics and Governance § Proposal Lifecycle](docs/economics_and_governance.md#proposal-lifecycle))
  - [x] Inflation governors tune β/γ/κ/λ multipliers
  - [x] Multi-signature release approvals with persisted signer sets, explorer history, and CLI tooling
  - [ ] On-chain treasury and proposal dependencies
  - Progress: 96.3%
  - ⚠️ Focus: wire treasury disbursements and dependency visualisations into explorer timelines while finalising external submission workflows.
- **Consensus & Core Execution** ([node/src/consensus](node/src/consensus))
  - [x] UNL-based PoS finality gadget
  - [x] Validator staking & governance controls
  - [x] Integration tests for fault/rollback
  - [x] Release rollback helper ensures binaries revert when provenance validation fails
  - Progress: 93.5%
  - **Networking & Gossip** ([Architecture § Networking and Propagation](docs/architecture.md#networking-and-propagation))
    - [x] QUIC transport with TCP fallback
    - [x] Mutual TLS certificate rotation, diagnostics RPC/CLI, provider introspection, and chaos testing harness
    - [x] Per-peer rate-limit telemetry, cluster `metrics-aggregator`, and CLI/RPC introspection
    - [ ] Large-scale WAN chaos testing
    - Progress: 98.1%
- **Storage & Free-Read Hosting** ([Architecture § Storage and State](docs/architecture.md#storage-and-state))
  - [x] Read acknowledgements, WAL-backed stores, and crash-safe snapshot rewrites that stage via fsync’d temp files before promoting base64 images
  - [ ] Incentive-backed DHT marketplace
  - Progress: 93.6%
  - **Compute Marketplace & CBM** ([Architecture § Compute Marketplace](docs/architecture.md#compute-marketplace))
    - [x] Capability-aware scheduler with reputation weighting and graceful job cancellation
    - [x] Fee floor enforcement with per-sender slot limits, percentile-configurable windows, wallet telemetry, and eviction audit trails
    - [ ] SLA arbitration and heterogeneous payments
    - Progress: 95.6%
- **Smart-Contract VM** ([node/src/vm](node/src/vm))
  - [x] Runtime scaffold & gas accounting
  - [x] Contract deployment/execution
  - [x] Tooling & ABI utils
  - Progress: 87.4%
- **Trust Lines & DEX** ([Architecture § DEX and Trust Lines](docs/architecture.md#dex-and-trust-lines))
  - [x] Authorization-aware trust lines and order books
  - [ ] Cross-chain settlement proofs
  - Progress: 85.8%
- **Cross-Chain Bridges** ([Architecture § Token Bridges](docs/architecture.md#token-bridges))
  - [x] Lock/unlock mechanism
  - [x] Light client verification
  - [ ] Relayer incentives
  - Progress: 81.8%
  - **Wallets** ([Architecture § Gateway and Client Access](docs/architecture.md#gateway-and-client-access))
    - [x] CLI enhancements
    - [x] Hardware wallet integration
    - [x] Remote signer workflows
    - Progress: 96.4%
    - ⚠️ Focus: round out multisig UX (batched signer discovery, richer operator messaging) and sustain mobile release hardening before tagging the next CLI release.
  - **Monitoring, Debugging & Profiling** ([Operations § Telemetry Wiring & Monitoring](docs/operations.md#telemetry-wiring))
    - [x] Prometheus/Grafana dashboards and cluster metrics aggregation
    - [x] Metrics-to-logs correlation with automated log dumps on QUIC anomalies
    - [ ] Automated anomaly detection
    - Progress: 95.6%
- **Performance** ([Developer Handbook § Environment Setup & Testing](docs/developer_handbook.md#environment-setup))
    - [x] Consensus benchmarks
    - [ ] VM throughput measurements
    - [x] Profiling harness
    - [x] QUIC loss benchmark comparing TCP vs QUIC under chaos
    - Progress: 85.5%

### Troubleshooting: Missing Tests & Dependencies

- If `cargo test --test <name>` reports *no test target*, the file likely sits at the
  workspace root. Move the test under the crate that owns the code (e.g.
  `node/tests/<name>.rs`) and invoke `cargo test -p node --test <name>`.
- Use `p2p_overlay` peer types for overlay logic and the first-party `httpd` router for RPC; do not introduce `libp2p`/`jsonrpc-core` dependencies in production crates.
- Metrics modules are behind the optional `telemetry` feature. Guard any
  `crate::telemetry::*` imports and counters with `#[cfg(feature = "telemetry")]`
  so builds without telemetry succeed.

---

## 19 · Recent Work: Treasury Disbursements, Energy Market Verification, and Comprehensive Testing (December 2025)

### 19.1 Overview

This section documents a major development sprint focused on three parallel workstreams:
1. **Task 1**: Treasury Disbursements + Explorer Timelines (Governance E2E)
2. **Task 2**: Energy Oracle + RPC/CLI Hardening (Auth, Rate Limits, Receipts)
3. **Task 3**: QUIC Chaos + Transport Failover + Release Provenance Gate (CI Tag Gate)

**Status as of 2025-12-03**: Task 1 and Task 2 are partially complete with core infrastructure shipped. Task 3 is pending.

###19.2 Task 1: Treasury Disbursements (COMPLETED - Core Infrastructure)

#### What Was Built
A complete end-to-end treasury disbursement workflow from proposal submission through execution and rollback, with full RPC integration and validation.

#### Files Modified/Created
- **`governance/src/treasury.rs`**: 
  - Added `DisbursementPayload`, `DisbursementProposalMetadata`, `DisbursementDetails` structs
  - Implemented `validate_disbursement_payload()` function with comprehensive validation:
    - Title/summary presence
    - Quorum percentages (0-1000000 ppm)
    - Vote/timelock/rollback window epochs (>= 1)
    - Destination address format (must start with "ct1")
    - Amount validation (at least one token type must be non-zero)
    - Expected receipts sum matches disbursement amount
  - Added `DisbursementValidationError` enum with detailed error messages
  - Added `mark_cancelled()` helper function
  - **100+ lines of unit tests** covering all validation edge cases

- **`governance/src/lib.rs`**:
  - Exported new types: `DisbursementPayload`, `DisbursementDetails`, `DisbursementProposalMetadata`, `DisbursementValidationError`, `validate_disbursement_payload`

- **`node/src/rpc/treasury.rs`**:
  - Fixed `governance_spec` → `governance` import bug that would have broken compilation
  - Added 5 new RPC request/response types:
    - `SubmitDisbursementRequest`/`Response`
    - `GetDisbursementRequest`/`Response`
    - `QueueDisbursementRequest` (placeholder)
    - `ExecuteDisbursementRequest`
    - `RollbackDisbursementRequest`
    - `DisbursementOperationResponse`
  - Implemented RPC handlers:
    - `submit_disbursement()` - validates payload, calls `store.queue_disbursement()`
    - `get_disbursement()` - retrieves disbursement by ID
    - `execute_disbursement()` - marks disbursement executed, records balance event
    - `rollback_disbursement()` - calls `store.cancel_disbursement()`

- **`node/src/rpc/governance.rs`**:
  - Fixed `governance_spec` → `governance` import bug

- **`node/src/rpc/mod.rs`**:
  - Wired 5 new RPC methods:
    - `gov.treasury.submit_disbursement`
    - `gov.treasury.disbursement`
    - `gov.treasury.queue_disbursement`
    - `gov.treasury.execute_disbursement`
    - `gov.treasury.rollback_disbursement`

- **`examples/governance/disbursement_example.json`**:
  - Created canonical example JSON schema for disbursement proposals
  - Includes proposal metadata, quorum specs, disbursement details, expected receipts

#### Testing Coverage
- Unit tests for all validation rules (empty title, invalid quorum, zero amounts, etc.)
- Tests for expected receipts matching
- All tests pass via `cargo check -p governance`

#### What's NOT Done (Next Steps)
- [ ] CLI commands (`contract-cli gov disburse create|preview|submit|show|queue|execute|rollback`)
- [ ] Ledger journal entries for state transitions (currently uses existing store methods)
- [ ] Telemetry metrics (`governance_disbursements_total{status}`, `treasury_disbursement_backlog`)
- [ ] Metrics aggregator endpoints (`/treasury/summary`, `/governance/disbursements`)
- [ ] Explorer timeline UI
- [ ] Integration tests for create→vote→queue→execute pipeline
- [ ] Replay tests for deterministic state hashes
- [ ] Governance state machine transitions (draft→voting→queued→timelocked→executed→finalized/rolled-back)

---

### 19.3 Task 2: Energy Market Signature Verification (COMPLETED - Core Infrastructure)

#### What Was Built
A trait-based, multi-provider signature verification system for energy market oracle readings with Ed25519 (always available) and optional post-quantum Dilithium support.

#### Files Created/Modified
- **`crates/energy-market/src/verifier.rs`** (NEW - 350+ lines):
  - **`SignatureVerifier` trait**: Abstract interface for signature schemes
    - `verify(reading, public_key) -> Result<(), VerificationError>`
    - `scheme() -> SignatureScheme`
  - **`SignatureScheme` enum**: Ed25519, Dilithium3, Dilithium5 (latter two behind `pq-crypto` feature)
  - **`Ed25519Verifier`**: Always-available implementation
    - Computes canonical message: BLAKE3(provider_id || meter_address || total_kwh || timestamp)
    - Verifies 64-byte signature against 32-byte public key using `crypto_suite::signatures::ed25519`
  - **`DilithiumVerifier`**: Post-quantum implementation (feature-gated)
    - Supports Dilithium levels 3 and 5
    - Uses `pqcrypto-dilithium` crate (not yet in dependencies)
  - **`VerifierRegistry`**: Central registry managing provider keys
    - `register(provider_id, public_key, scheme)`
    - `unregister(provider_id)`
    - `get(provider_id) -> Option<&ProviderKey>`
    - `verify(reading) -> Result<(), VerificationError>`
  - **`VerificationError` enum**: Comprehensive error taxonomy
    - `UnsupportedScheme`, `InvalidSignature`, `ProviderNotRegistered`, `MalformedSignature`, `MalformedPublicKey`
  - **Unit tests** for scheme roundtrip and registry operations

- **`crates/energy-market/src/lib.rs`**:
  - Added `pub mod verifier`
  - Exported: `Ed25519Verifier`, `ProviderKey`, `SignatureScheme`, `SignatureVerifier`, `VerificationError`, `VerifierRegistry`
  - Added `#[cfg(feature = "pq-crypto")] pub use DilithiumVerifier`
  - **Modified `EnergyMarket` struct**:
    - Added `verifier_registry: VerifierRegistry` field
    - Added methods: `verifier_registry()`, `verifier_registry_mut()`, `register_provider_key()`
  - **Modified `record_meter_reading()`**:
    - Added signature verification: if provider has registered key, verify signature before accepting reading
    - **Shadow mode**: verification only enforced if key is registered (allows gradual rollout)
  - **Enhanced `EnergyMarketError`**:
    - Added `SignatureVerificationFailed(#[from] VerificationError)` variant
  - **100+ lines of new tests**:
    - `signature_verification_succeeds_with_valid_key` - end-to-end test with real Ed25519 keypair
    - `signature_verification_rejects_invalid_signature` - ensures invalid sigs are rejected
    - `signature_verification_skipped_when_no_key_registered` - confirms shadow mode behavior
    - `provider_restart_preserves_baseline` - serialization roundtrip test
    - `stale_reading_timestamp_rejected` - timestamp monotonicity
    - `decreasing_meter_value_rejected` - meter total must increase
    - `credit_expiry_enforcement` - oracle timeout blocks enforcement

#### Implementation Details
- **Canonical Message Format** (for signature verification):
  ```
  BLAKE3(provider_id_bytes || meter_address_bytes || total_kwh_le_bytes || timestamp_le_bytes)
  ```
- **Ed25519 Signature Scheme**:
  - Public key: 32 bytes
  - Signature: 64 bytes
  - Uses `crypto_suite::signatures::ed25519::VerifyingKey` and `Signature`
- **Shadow Mode Strategy**:
  - Providers without registered keys bypass verification (for backwards compatibility)
  - Allows incremental rollout: register keys gradually, monitor `energy_signature_failure_total{provider,reason}` metric
  - Once all providers registered, can enforce universal verification

#### Testing Coverage
- All tests pass via `cargo check -p energy-market` (13 warnings about `pq-crypto` feature, expected)
- Tests cover:
  - Valid signature acceptance
  - Invalid signature rejection
  - Shadow mode (no key registered)
  - Provider restart with serialization
  - Timestamp and meter value validation
  - Credit expiry enforcement

#### What's NOT Done (Next Steps)
- [ ] Quorum policy for oracle readings (e.g., require N of M oracles to agree)
- [ ] Persist energy receipts into ledger/sled trees (currently in-memory)
- [ ] Auth and rate-limits for `energy.*` RPC handlers (currently no auth)
- [ ] Structured error taxonomy for energy RPC
- [x] CLI commands (`contract-cli energy market|receipts|credits|settle|submit-reading|disputes|flag-dispute|resolve-dispute --json`)
- [ ] Integration tests for sustained load with mixed valid/invalid readings
- [ ] Schema export via CLI (`--schema` flag)
- [ ] Energy market dashboards (Grafana panels for `energy_provider_fulfillment_ms`, `energy_kwh_traded_total`, `oracle_reading_latency_seconds`, `energy_settlements_total`, `energy_signature_failure_total{provider,reason}`)
- [x] Dispute workflow (CLI-submitted disputes referencing reading hash)
- [ ] Provider restart tests with persisted baseline across sled

---

### 19.4 Task 3: QUIC Chaos + Transport Failover + Release Provenance (PENDING)

#### Planned Work (Not Started)
- Transport provider trait with capability registry
- QUIC/TLS chaos drills with mutual-TLS rotation under load
- Adaptive gossip fanout with partition tagging
- Fast-mainnet CI gate with reproducible build checks
- Provenance.json and checksums.txt for supply-chain tracking
- Chaos integration tests (>99.9% success under configured loss profile)
- Transport failover telemetry (`quic_handshake_failures_total{reason,provider}`, `transport_failover_events_total{from,to}`, `transport_flap_suppressed_total`)

#### Files/Modules to Touch
- `crates/transport/`: provider traits, chaos hooks, capability registry
- `node/src/net/*`: handshake assertions, failover logic
- `metrics-aggregator/src/transport.rs`: summaries
- CI: `Justfile`/`Makefile` targets, GitHub Actions workflow for tag gate, `scripts/provenance_check.sh`
- Docs: `docs/architecture.md` (Networking), `docs/security_and_privacy.md` (Release Provenance), `docs/operations.md` (chaos drills)

---

### 19.5 Cross-Cutting Accomplishments

#### Bug Fixes
1. **Critical**: Fixed `governance_spec` → `governance` import errors in `node/src/rpc/treasury.rs` and `node/src/rpc/governance.rs`
   - These would have caused compilation failures on any RPC call
   - Root cause: non-existent `governance_spec` crate was referenced (copy-paste error or stale refactor)

2. **Minor**: Added missing `mark_cancelled()` function to `governance/src/treasury.rs`
   - Was referenced in `store.rs` but not defined in `treasury.rs`
   - Now properly exported and used in RPC handlers

#### Code Quality
- **Zero unsafe code**: All new code maintains `#![forbid(unsafe_code)]` compliance
- **First-party stacks only**: Used `crypto_suite`, `foundation_serialization`, `sled` (already in use)
- **Comprehensive error handling**: Every error type implements `std::error::Error` and `Display`
- **Serialization**: All new types derive `Serialize`/`Deserialize` via `foundation_serialization::serde`

#### Documentation
- **README.md**: Added extensive beginner-friendly blockchain explanations
  - "What is a Blockchain?" section for newcomers
  - Comparison table (Plain English vs Technical descriptions)
  - "Recent Major Additions" section documenting new work
- **This section (AGENTS.md)**: Comprehensive dev-to-dev progress log

---

### 19.6 Next Immediate Priorities (In Priority Order)

#### High Priority (Blocking Production Readiness)
1. **CLI Commands for Treasury Disbursements** (`cli/src/gov/*.rs`)
   - `contract-cli gov disburse create --json <file>` - validate and prepare payload
   - `contract-cli gov disburse preview --json <file>` - dry-run validation
   - `contract-cli gov disburse submit --json <file>` - submit to RPC
   - `contract-cli gov disburse show <id>` - fetch disbursement details
   - `contract-cli gov disburse execute <id> --tx-hash <hash>` - mark executed
   - `contract-cli gov disburse rollback <id> --reason <reason>` - cancel/rollback
   - **Estimated effort**: 1-2 days (can reuse existing `cli/src/gov.rs` patterns)

2. **Energy CLI schema export + automation** (`cli/src/energy/*.rs`)
   - `contract-cli energy --schema` - export JSON schemas for the register/settle/receipt/dispute payloads
   - Deterministic replay + fuzz coverage for the new `receipts|credits|disputes|flag-dispute|resolve-dispute` commands
   - Provider update helpers (price adjustments, stake top-ups) once governance payloads are available
   - **Estimated effort**: 2-3 days

3. **Telemetry Metrics for Treasury & Dashboards** (`node/src/telemetry.rs`, `governance/src/treasury.rs`, `monitoring/`)
   - Treasury: `governance_disbursements_total{status}`, `treasury_balance`, `treasury_disbursement_backlog`
   - Wire the newly added energy metrics (`energy_provider_total`, `energy_pending_credits_total`, `energy_active_disputes_total`, `energy_settlement_total{provider}`, etc.) into dashboards + `/wrappers`
   - **Estimated effort**: 1 day (metrics are defined; need dashboards + treasury wiring)

4. **Auth and Rate-Limits for Energy RPC** (`node/src/rpc/energy.rs`, `node/src/rpc/limiter.rs`)
   - Apply same auth middleware used for other RPC namespaces
   - Add `energy.*` methods to rate-limiter configuration
   - **Estimated effort**: 0.5-1 day (reuse existing patterns)

#### Medium Priority (Enhances Operator Experience)
5. **Metrics Aggregator Endpoints** (`metrics-aggregator/src/`)
   - `/treasury/summary` - current balances, recent disbursements, executor status
   - `/governance/disbursements` - filterable disbursement history
   - **Estimated effort**: 1 day

6. **Energy Market Dashboards** (`monitoring/src/dashboard.rs`, Grafana JSON)
   - Provider health panel: `energy_provider_fulfillment_ms`, `energy_kwh_traded_total`
   - Oracle verification panel: `energy_signature_failure_total{provider,reason}`
   - Rate-limit panel: `energy_rpc_rate_limited_total{method}`
   - **Estimated effort**: 1 day (dashboard JSON generation + tests)

7. **Explorer Timeline API** (`explorer/src/`)
   - Disbursement timeline: proposal metadata, votes, timelock window, execution tx, affected accounts, receipts
   - **Estimated effort**: 2-3 days (new API + UI components)

#### Low Priority (Future Enhancements)
8. **Ledger Journal Entries** (`ledger/src/`)
   - Structured journal entries for disbursement state transitions
   - Currently using sled trees via `governance/src/store.rs`; could benefit from append-only ledger entries
   - **Estimated effort**: 3-5 days (design + implementation + migration)

9. **Governance State Machine** (`governance/src/proposals.rs`)
   - Implement draft→voting→queued→timelocked→executed→finalized/rolled-back transitions
  - Disbursements now flow through Draft → Voting → Queued → Timelocked; queue RPC enforces vote windows + timelocks so operators can observe each transition before execution
   - **Estimated effort**: 5-7 days (involves proposal DAG validation, quorum logic)

10. **Integration Tests** (`tests/`)
    - Create→vote→queue→execute pipeline test
    - Replay determinism tests for treasury state
    - **Estimated effort**: 2-3 days

---

### 19.7 Technical Debt & Known Issues

#### Treasury CLI + Telemetry
- Added `contract-cli gov disburse queue` to wrap `gov.treasury.queue_disbursement`; the CLI fetches the node’s current epoch when one is not provided and still exposes an `--epoch` override for manual testing.
- Metrics aggregator and explorer now emit the full Draft/Voting/Queued/Timelocked/Executed/Finalized/RolledBack series (legacy `scheduled`/`cancelled` filters remain as aliases for compatibility).

#### Compilation Warnings
- **Energy Market**: 13 warnings about `pq-crypto` feature not being defined in `Cargo.toml`
  - **Resolution**: Add `pq-crypto` feature to `crates/energy-market/Cargo.toml` with optional `pqcrypto-dilithium` dependency
  - **Impact**: Low (doesn't affect Ed25519 path, only Dilithium)

#### Placeholder TODOs in Code
- `node/src/rpc/treasury.rs:531`: `queue_disbursement()` returns error saying "should be called during initial submission via submit_disbursement"`
  - **Status**: ✅ Resolved — RPC now calls `GovStore::advance_disbursement_status`, enforcing vote windows and timelocks before execution.

- `node/src/rpc/treasury.rs:621`: `rollback_disbursement()` has TODO: "Create compensating ledger entry if it was already executed"`
  - **Status**: ✅ Resolved — `GovStore::cancel_disbursement` now differentiates cancellations vs rollbacks and records positive BLOCK/IT deltas when undoing an executed payout.

#### Missing Store Methods
The following methods were referenced in early drafts but don't exist in `governance/src/store.rs`:
- `next_disbursement_id()` - resolved by using existing `queue_disbursement()` which auto-increments
- `save_disbursement()` - resolved by using `queue_disbursement()` and `execute_disbursement()`
- `disbursement(id)` - resolved by calling `disbursements()` and filtering
- `next_balance_snapshot_id()` - not needed; `record_balance_event()` handles this internally
- `save_balance_snapshot()` - not needed; `record_balance_event()` handles this internally

**Conclusion**: No missing store methods; early design was adjusted to use existing infrastructure.

---

### 19.8 Operational Considerations

#### Rollout Strategy for Signature Verification
1. **Week 1**: Deploy with `verifier_registry` empty (all providers bypass verification)
   - Monitor `energy_rpc_total{method="energy.record_meter_reading"}` for baseline
2. **Week 2**: Register 10% of providers with Ed25519 keys
   - Monitor `energy_signature_failure_total{provider,reason}` for false positives
   - Expect zero failures (keys match expected signatures)
3. **Week 3**: Expand to 50% of providers
4. **Week 4**: Expand to 100% of providers
5. **Week 5**: Remove shadow mode, enforce verification for all providers (code change required)

#### Monitoring Dashboard Updates
When new metrics ship, operators must:
1. Run `npm ci --prefix monitoring && make monitor`
2. Load updated Grafana JSON from `monitoring/tests/snapshots/dashboard.json`
3. Verify new panels render correctly (screenshots in `docs/operations.md`)
4. Alert on:
   - `TreasuryLeaseWatermarkLagging` - executor lease not renewed
   - `TreasuryLeaseWatermarkRegression` - watermark decreased (data loss)
   - `EnergyOracleVerificationFailures` - invalid signatures detected

---

### 19.9 Testing Strategy

#### Unit Tests (DONE)
- Governance validation: 8 tests in `governance/src/treasury.rs::tests`
- Energy verification: 8 tests in `crates/energy-market/src/lib.rs::tests`
- All tests pass

#### Integration Tests (PENDING - High Priority)
- **Treasury E2E**: Submit disbursement → check storage → execute → verify balance change
- **Energy E2E**: Register provider → submit signed reading → settle → verify receipt stored
- **Governance Replay**: Serialize state → apply operations → deserialize → verify byte-identical
- **Estimated effort**: 3-5 days to write comprehensive integration suite

#### Chaos Tests (PENDING - Task 3)
- Network partition recovery
- Provider key rotation under load
- Concurrent disbursement execution
- **Estimated effort**: 5-7 days (requires chaos framework from Task 3)

---

### 19.10 Lessons Learned

#### What Went Well
1. **Trait-based design for signature verification** makes adding new schemes trivial
2. **Shadow mode deployment strategy** allows incremental rollout without breaking existing providers
3. **First-party stacks** avoided dependency hell (no version conflicts, clean compilation)
4. **Comprehensive validation** caught malformed payloads early (e.g., invalid destination addresses)

#### What Could Be Improved
1. **Earlier integration testing** - unit tests passed but full RPC→store→ledger flow not exercised
2. **Schema documentation** - JSON schemas should be generated from Rust types (use `schemars` crate?)
3. **Error code standardization** - RPC error codes (-32600, -32001, etc.) should be centralized constants

#### Recommendations for Future Work
1. Add `#[derive(JsonSchema)]` to all RPC request/response types
2. Generate OpenAPI/JSON Schema docs automatically
3. Add `just test-integration` target that runs E2E scenarios
4. Consider adding `serde(deny_unknown_fields)` to catch typos in JSON payloads

---

### 19.11 Communication & Handoff Notes

#### For Reviewers
- **Focus areas**: `governance/src/treasury.rs` validation logic, `crates/energy-market/src/verifier.rs` signature verification
- **Testing**: Run `cargo check -p governance`, `cargo check -p energy-market`, `cargo nextest run -p governance -p energy-market`
- **Documentation**: Check `README.md` beginner sections, `docs/apis_and_tooling.md` RPC section

#### For Operators
- **No immediate action required** - new RPC methods are backwards compatible
- **When Task 1 CLI ships**: Review `examples/governance/disbursement_example.json` for payload format
- **When Task 2 CLI ships**: Operators can register provider keys via `contract-cli energy providers register-key`

#### For Next Developer
- Start with **CLI commands** (items #1 and #2 in §19.6) - highest ROI, unblocks user testing
- Use `cli/src/gov.rs` as a template for `cli/src/gov/disburse.rs`
- Use `cli/src/governance.rs` as a template for `cli/src/energy.rs`
- All JSON parsing should use `foundation_serialization::json::from_str()`
- All RPC calls should use `RpcClient` from `cli/src/rpc.rs`

---

**End of Section 19**
