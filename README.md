# The Block

## For Blockchain Newcomers: What is This Project?

**The Block** is a blockchain - think of it as a special kind of database that nobody owns, nobody can tamper with, and everyone can verify. Here's what makes blockchains different from regular databases:

### What is a Blockchain?
Imagine a notebook that records every transaction (like "Alice sent 5 coins to Bob"). In a traditional bank, the bank keeps this notebook and you trust them not to cheat. In a blockchain:
1. **Everyone has a copy** of the notebook (distributed)
2. **New pages are added** by solving hard math problems (mining/proof-of-work)
3. **Each page references the previous page**, forming a "chain" of blocks
4. **Nobody can change old pages** because everyone would notice the mismatch

### What is The Block Specifically?

The Block is a **Layer 1 (L1) blockchain**, meaning it's a foundation blockchain (like Bitcoin or Ethereum) rather than something built on top of another blockchain. Think of L1s as the bedrock - they provide the fundamental security and consensus.

**Key Features for Beginners:**
- **Proof-of-Work (PoW)**: Like Bitcoin, miners compete to solve puzzles to add new blocks. This makes attacking the network extremely expensive.
- **Proof-of-Service**: Unlike Bitcoin which only rewards miners, The Block also rewards people who provide useful services like:
  - **Storage** (keeping files safe)
  - **Computation** (running programs)
  - **Bandwidth** (helping data move around)
- **Targeted Ad Marketplace**: A built-in ad market matches campaigns to viewers using privacy-preserving “cohorts” defined by domain tiers, badges, interest tags, and proof-of-presence buckets sourced from LocalNet/Range Boost infrastructure. Governance controls every selector knob, and readiness/auction metrics are wired into Grafana so operators can prove the system is production-ready before mainnet.
- **Single Currency (CT - Consumer Token)**: You can send and receive CT just like Bitcoin or cash, but it also pays for services on the network.
- **Governance**: Instead of a small group deciding how the blockchain works, CT holders can vote on proposals to change rules, distribute funds from the treasury, and upgrade the network.

### Why "Self-Contained" Matters

Most blockchains piece together libraries from dozens of different teams. If one library has a bug or gets abandoned, the whole thing can break. The Block is different:
- **All code lives in this repository** - from the low-level networking to the user interface
- **Written in Rust** - a programming language that prevents many types of bugs and security issues
- **First-party everything** - HTTP servers, databases, serialization, cryptography all built by the same team with the same standards

This means when you run The Block, you're not duct-taping together 50 different projects - you're running one cohesive, auditable system.

---

## Technical Summary

The Block is a Rust-first, proof-of-work + proof-of-service L1 that mints a single transferable currency (CT), notarises micro-shard roots every second, and ships every critical component in-repo. Transport, HTTP/TLS, serialization, overlay, storage, governance, CLI, explorer, metrics, and tooling all share the same first-party stacks so operators can run a full cluster without third-party glue. The newest tranche of work extends the ad marketplace into a multi-signal targeting engine (domain tiers, interest tags, presence attestations) while keeping privacy budgets, telemetry, and governance knobs front-and-center.

---

## Why it exists

### For Beginners: The Vision
Most blockchains focus only on sending money around. The Block goes further - it rewards people for providing **useful services** to the network:
- **Storage providers** keep your files safe (like Dropbox, but decentralized)
- **Compute providers** run programs for you (like AWS, but decentralized)
- **Energy market** lets you buy and sell real-world electricity with built-in verification
  - Configure oracle trust roots directly in `config/default.toml` via `energy.provider_keys` (each entry maps a provider ID to a 32-byte Ed25519 public key). Reloading the config hot-swaps the verifier registry so you can roll keys without restarting nodes.
  - Meter submissions are rejected unless they carry a valid Ed25519 signature over the canonical payload (`MeterReadingPayload::signing_bytes`). Telemetry surfaces failures via `energy_signature_failure_total{provider,reason}` so operators can alert on bad or missing signatures.
  - Dispute workflows now live behind first-party RPCs (`energy.disputes`, `energy.flag_dispute`, `energy.resolve_dispute`, `energy.receipts`, `energy.credits`) and the matching CLI (`tb-cli energy disputes|receipts|credits|flag-dispute|resolve-dispute`). Operators can page through outstanding credits/receipts, flag a `meter_hash`, and record resolutions without spelunking sled snapshots or pushing ad-hoc governance proposals.
  - Energy telemetry exports provider/credit/dispute gauges (`energy_provider_total`, `energy_pending_credits_total`, `energy_receipt_total`, `energy_active_disputes_total`) plus counters (`energy_provider_register_total`, `energy_meter_reading_total{provider}`, `energy_settlement_total{provider}`, `energy_treasury_fee_ct_total`, `energy_dispute_{open,resolve}_total`). Dashboards wire these straight into Grafana via the metrics-aggregator.

Instead of paying these providers per request with transaction fees (which gets expensive fast), The Block pays them automatically when new blocks are mined - similar to how Bitcoin pays miners, but for many types of useful work.

### For Developers: Design Pillars
- **Reward verifiable service:** storage/compute/bandwidth subsidies (`STORAGE_SUB_CT`, `READ_SUB_CT`, `COMPUTE_SUB_CT`) are minted directly in each coinbase instead of billing per request. This eliminates micro-transaction overhead and makes services economically viable.

- **Deterministic governance:** the shared `governance` crate powers the node, CLI, explorer, and metrics aggregator so proposals, fee floors, and treasury state never drift. Every participant runs identical governance logic, ensuring the network can coordinate upgrades and treasury disbursements without hard forks.

- **Reproducible, sovereign builds:** dependency snapshots via `cargo vendor`, first-party `foundation_serialization`, and provenance tracking keep binaries auditable. Telemetry advertises exactly which runtime/transport/storage/coding providers are active, so node operators can verify they're running authentic software.

- **First-party tooling:** gateway HTTP, DNS publishing, CLI, probe, explorer, light clients, and telemetry stacks all reuse the same crates. No vendor lock-in, no mystery dependencies - everything is built, tested, and shipped together.

---

## What's inside this repo?

### For Beginners: Repository Structure
Think of this repository like a city with different neighborhoods:

| Area | What It Does (Plain English) | What It Does (Technical) |
| --- | --- | --- |
| **`node/`** | The main blockchain software that validates transactions, mines blocks, and talks to other nodes | Full node, gateway, mempool, compute/storage pipelines, RPC, light-client streaming, telemetry. `#![forbid(unsafe_code)]` by default. |
| **`crates/`** | Shared libraries (building blocks) used by multiple parts of the system | Reusable libraries: `transport`, `httpd`, `foundation_*`, `storage_engine`, `p2p_overlay`, `wallet`, `probe`, etc. |
| **`cli/`** | Command-line tool (`tb-cli`) - like a control panel for interacting with the blockchain | Handles governance, wallet, bridge, compute market, storage, telemetry, diagnostics, and remediation flows. |
| **`governance/`** | Rules and voting system - how the network makes decisions and manages the treasury | Bicameral voting, treasury disbursements, parameter adjustments, release attestations |
| **`crates/energy-market/`** | NEW! Energy trading marketplace with oracle-verified meter readings and multi-scheme signature verification (Ed25519 + optional post-quantum) | Providers register, meters submit signed readings, buyers settle against credits, receipts stored in ledger |
| **`metrics-aggregator/` + `monitoring/`** | Dashboard and monitoring - shows you what's happening on the network in real-time | Aggregates `/metrics`, exposes `/wrappers`, `/treasury`, `/governance`, `/probe`, `/chaos`, `/remediation/*`, and feeds Grafana dashboards. |
| **Tooling** (`scripts/`, `tools/`, `examples/`, `sim/`, `fuzz/`, `formal/`) | Scripts and tests to make sure everything works correctly | Bootstrap scripts, dependency policy tooling, settlement/replay harnesses, chaos testing, fuzzing, and formal verification inputs. |

### Recent Major Additions
- **Treasury Disbursement System**: Complete end-to-end workflow for governance-approved fund distributions with RPC handlers (`gov.treasury.submit_disbursement`, `execute_disbursement`, `rollback_disbursement`) and full validation
- **Disbursement Status Machine**: Queue/timelock/rollback logic now flows through `gov.treasury.queue_disbursement` (driven from `tb-cli gov disburse queue`, which auto-derives the current epoch), and metrics/explorer surfaces now emit the full Draft → Voting → Queued → Timelocked → Executed/Finalized/RolledBack labels so operators can see exactly where each payout sits before execution
- **Energy Market Signature Verification**: Trait-based multi-provider signature system with Ed25519 (always available) and Dilithium (post-quantum, feature-gated), enabling oracle meter readings to be cryptographically verified
- **Comprehensive Testing**: 100+ new unit tests covering signature verification, credit persistence across provider restarts, oracle timeout enforcement, and disbursement validation

See [`docs/overview.md`](docs/overview.md#document-map) for the authoritative document map.

---

## Quick start (one-time setup)
1. **Install prerequisites**: Rust 1.86+, `cargo-nextest`, `cargo-fuzz` (nightly), Python 3.12.3, Node 18+. On Linux make sure `patchelf` is available.
2. **Bootstrap toolchains and the virtualenv**:
   ```bash
   ./scripts/bootstrap.sh
   ```
   The script wires the Python shim, vendors dependencies, and validates your environment.
3. **Lint, format, and run fast tests** (mirrors CI defaults):
   ```bash
   just lint && just fmt && just test-fast
   ```
4. **Run the full feature set** (telemetry + release checks):
   ```bash
   just test-full
   ```
5. **Start a local node**:
   ```bash
   tb-cli node start --config node/.env.example
   ```
   Environment variables use the `TB_*` namespace; inspect `node/src/config.rs` for every knob.

---

## Common workflows
- **Workspace test sweep:** `cargo nextest run --all-features` exercises node, CLI, crates, and the metrics aggregator. Replay determinism lives in `cargo test -p the_block --test replay`; settlement audits run via `cargo test -p the_block --test settlement_audit --release`.
- **Fuzzing & coverage:** `scripts/fuzz_coverage.sh` installs LLVM tooling, runs fuzz targets, and exports `.profraw` coverage.
- **Monitoring stack:** `npm ci --prefix monitoring && make monitor` builds Grafana/Prometheus bundles driven by the metrics aggregator.
- **Docs:** `mdbook build docs` renders everything under `docs/` with the configuration in `docs/book.toml`. Refer to [`docs/LEGACY_MAPPING.md`](docs/LEGACY_MAPPING.md) if you are trying to track where older specs moved.
- **Dependency policy:** `cargo run -p dependency_registry -- --check config/dependency_policies.toml` refreshes `docs/dependency_inventory*.json`.

---

## Documentation guide

| Document | Why you should read it |
| --- | --- |
| [`docs/overview.md`](docs/overview.md) | Mission, design pillars, repo layout, document map. |
| [`docs/architecture.md`](docs/architecture.md) | Ledger & consensus, networking, storage, compute marketplace, bridges/DEX, gateway, telemetry. |
| [`docs/economics_and_governance.md`](docs/economics_and_governance.md) | CT supply, fee lanes, subsidy multipliers, treasury, governance DAG, settlement math. |
| [`docs/operations.md`](docs/operations.md) | Bootstrap, configuration, telemetry wiring, runbooks, probe/diagnostics, WAL/snapshot care, deployments. |
| [`docs/security_and_privacy.md`](docs/security_and_privacy.md) | Threat model, crypto stack, remote signers, jurisdiction packs, LE portal, supply-chain security. |
| [`docs/developer_handbook.md`](docs/developer_handbook.md) | Environment setup, coding standards, testing/fuzzing, simulation, dependency policy, WASM/contracts, contribution flow. |
| [`docs/apis_and_tooling.md`](docs/apis_and_tooling.md) | JSON-RPC, CLI, gateway HTTP & DNS, explorer, light-client streaming, storage APIs, probe CLI, metrics schemas. |

Everything is kept in sync with `mdbook`; CI blocks merges if documentation drifts from the implementation.

---

## Contributing & support
- **New contributors:** read [`AGENTS.md`](AGENTS.md) once and work like you authored it. It documents expectations, testing gates, release policy, escalation paths, and the Document Map owners you must loop in before touching each subsystem. The spec-first contract applies even to README/handbook updates—patch docs first, cite them in your PR, then change code.
- **Ad-market track:** With the cohort/presence spec locked in this README + `docs/`, implementation now moves to the backlog in `AGENTS.md §15.K`. Code/CLI/RPC changes must cite the doc sections updated here and include the readiness checklist artifacts (`docs/overview.md#ad--targeting-readiness-checklist`).
- **Repeatable workflows:** prefer `just` targets and the scripts under `scripts/` instead of ad-hoc commands. Hook `scripts/pre-commit.sample` if you want automatic fmt/lint before each commit.
- **Issues/questions:** open an issue describing the doc/code mismatch before changing behavior—spec drift is treated as a bug.
- **Licensing:** Apache 2.0 (`LICENSE`) governs the entire repo, including generated artifacts.

When in doubt, update the docs first, then patch the code. Production readiness is assumed at every commit—tests, reproducible builds, and telemetry are not optional.
