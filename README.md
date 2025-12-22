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
- **Targeted Ad Marketplace**: A built-in ad market matches campaigns to viewers using privacy-preserving "cohorts" defined by domain tiers, badges, interest tags, and proof-of-presence buckets sourced from LocalNet/Range Boost infrastructure. Governance controls every selector knob, and readiness/auction metrics are wired into Grafana so operators can prove the system is production-ready before mainnet.
- **Single Currency (BLOCK)**: Fixed supply of 40 million BLOCK tokens (similar to Bitcoin's 21M cap). You can send and receive BLOCK like Bitcoin or cash, and it pays for all services on the network. Formula-driven issuance based on network activity - no arbitrary constants.
- **Launch Governor Autopilot**: A readiness controller watches block smoothness, replay success, peer liveness, DNS auction health, and (soon) per-market telemetry. It flips gates like `operational` and `naming` only after streak-based thresholds are met, persists signed decision snapshots, and lets operators keep testnet/mainnet runs auditable without manual babysitting.
- **Governance**: Instead of a small group deciding how the blockchain works, BLOCK holders can vote on proposals to change rules, distribute funds from the treasury, and upgrade the network.

### BLOCK in Everyday Terms

BLOCK is the single currency that powers everything on The Block. Think of it like money that can:
- **Transfer** like cash — send BLOCK to anyone, anywhere
- **Pay for services** — storage, compute, energy
- **Reward work** — earn BLOCK by running infrastructure

**Supply Cap:** 40 million BLOCK maximum, with formula-driven issuance that responds to network activity. There is no premine: genesis starts at zero emission, and every BLOCK in circulation is minted through the public reward formula.

**Mini-stories showing how BLOCK moves around:**

| Person | What They Do | BLOCK Flow |
|--------|--------------|---------|
| **Alice** | Runs a node that validates transactions | Earns BLOCK from block rewards (mining) |
| **Bob** | Stores important files on the network | Pays BLOCK for storage; providers earn that BLOCK |
| **Carol** | Offers spare compute power (like a mini AWS) | Earns BLOCK when people run jobs on her machine |
| **Dave** | Operates a smart meter that reports energy usage | Earns BLOCK for verified energy readings via the energy market |
| **Eve** | Wants to run a machine-learning model | Pays BLOCK to Carol's compute; gets results back |

**Note on Legacy Code:** You may see references to "CT" or "Consumer Token" in older code/docs - these now refer to BLOCK. The dual-token system (CT/IT) has been consolidated into a single BLOCK token.

### Why "Self-Contained" Matters

Most blockchains piece together libraries from dozens of different teams. If one library has a bug or gets abandoned, the whole thing can break. The Block is different:
- **All code lives in this repository** - from the low-level networking to the user interface
- **Written in Rust** - a programming language that prevents many types of bugs and security issues
- **First-party everything** - HTTP servers, databases, serialization, cryptography all built by the same team with the same standards

This means when you run The Block, you're not duct-taping together 50 different projects - you're running one cohesive, auditable system.

---

## Technical Summary

The Block is a Rust-first, proof-of-work + proof-of-service L1 that mints a single transferable currency (BLOCK), notarises micro-shard roots every second, and ships every critical component in-repo. Transport, HTTP/TLS, serialization, overlay, storage, governance, CLI, explorer, metrics, and tooling all share the same first-party stacks so operators can run a full cluster without third-party glue. The newest tranche of work extends the ad marketplace into a multi-signal targeting engine (domain tiers, interest tags, presence attestations) while keeping privacy budgets, telemetry, and governance knobs front-and-center.

**Readiness autopilot:** The Launch Governor (`node/src/launch_governor`) consumes chain + DNS telemetry (and soon economics/market metrics) to drive testnet and mainnet gating. It records signed decisions, enforces streak-based enter/exit thresholds, and ties into governance runtime flags so feature transitions are reproducible and reviewable.
Operators can now run `tb-cli governor status` to see the persisted `EconomicsPrevMetric` snapshot plus the telemetry gauges it mirrors (`economics_prev_market_metrics_{utilization,provider_margin}_ppm`), giving a single view that ties the JSON-RPC payload to the Prometheus series you use in Grafana.

---

## Why it exists

### For Beginners: The Vision
Most blockchains focus only on sending money around. The Block goes further - it rewards people for providing **useful services** to the network:
- **Storage providers** keep your files safe (like Dropbox, but decentralized)
- **Compute providers** run programs for you (like AWS, but decentralized)
- **Energy market** lets you buy and sell real-world electricity with built-in verification

Instead of paying these providers per request with transaction fees (which gets expensive fast), The Block pays them automatically when new blocks are mined - similar to how Bitcoin pays miners, but for many types of useful work.

### For Developers: Design Pillars
- **Reward verifiable service:** storage/compute/bandwidth subsidies (`STORAGE_SUB_CT`, `READ_SUB_CT`, `COMPUTE_SUB_CT`) are minted directly in each coinbase instead of billing per request. This eliminates micro-transaction overhead and makes services economically viable.

- **Formula-driven monetary policy:** block rewards come from a single network-activity formula shared by the node, CLI, explorer, and telemetry (`docs/economics_and_governance.md#network-driven-block-issuance`). Governance can adjust smoothing bounds and baselines, but there are no hidden constants or manual issuance tweaks.
  The same telemetry counters (`economics_epoch_tx_count`, `economics_epoch_tx_volume_block`, `economics_epoch_treasury_inflow_block`, plus `economics_block_reward_per_block`) feed Launch Governor's autopilot so testnet/mainnet promotions happen when throughput, volume, and treasury inflow all meet policy thresholds.

- **Deterministic governance:** the shared `governance` crate powers the node, CLI, explorer, and metrics aggregator so proposals, fee floors, and treasury state never drift. Every participant runs identical governance logic, ensuring the network can coordinate upgrades and treasury disbursements without hard forks.

- **Reproducible, sovereign builds:** dependency snapshots via `cargo vendor`, first-party `foundation_serialization`, and provenance tracking keep binaries auditable. Telemetry advertises exactly which runtime/transport/storage/coding providers are active, so node operators can verify they're running authentic software.

- **First-party tooling:** gateway HTTP, DNS publishing, CLI, probe, explorer, light clients, and telemetry stacks all reuse the same crates. No vendor lock-in, no mystery dependencies - everything is built, tested, and shipped together.

- **Automated readiness gates:** the Launch Governor evaluates chain, DNS, and market telemetry to enable or disable subsystems (`operational`, `naming`, upcoming economics/market gates). Every transition is streak-gated, timelocked, and recorded, so “testnet that runs itself” is auditable.

---

## What's inside this repo?

### For Beginners: Repository Structure
Think of this repository like a city with different neighborhoods:

| Area | What It Does (Plain English) | What It Does (Technical) |
| --- | --- | --- |
| **`node/`** | The main blockchain software that validates transactions, mines blocks, and talks to other nodes | Full node, gateway, mempool, compute/storage pipelines, RPC, light-client streaming, telemetry. `#![forbid(unsafe_code)]` by default. |
| **`crates/`** | Shared libraries (building blocks) used by multiple parts of the system | Reusable libraries: `transport`, `httpd`, `foundation_*`, `storage_engine`, `p2p_overlay`, `wallet`, `probe`, etc. |
| **`cli/`** | Command-line tool (`contract-cli`) - like a control panel for interacting with the blockchain | Handles governance, wallet, bridge, compute market, storage, telemetry, diagnostics, and remediation flows. |
| **`governance/`** | Rules and voting system - how the network makes decisions and manages the treasury | Bicameral voting, treasury disbursements, parameter adjustments, release attestations |
| **`crates/energy-market/`** | NEW! Energy trading marketplace with oracle-verified meter readings and multi-scheme signature verification (Ed25519 + optional post-quantum) | Providers register, meters submit signed readings, buyers settle against credits, receipts stored in ledger |
| **`crates/ad_market/`** | Privacy-aware advertising system — groups users into broad "cohorts" (site type, badges, approximate presence), not individual tracking. Advertisers bid on cohorts, not people. | Domain tiers, interest tags, presence buckets, privacy budgets, uplift experiments. See [`node/src/ad_policy_snapshot.rs`](node/src/ad_policy_snapshot.rs), [`node/src/ad_readiness.rs`](node/src/ad_readiness.rs), [`cli/src/ad_market.rs`](cli/src/ad_market.rs), [`docs/architecture.md#ad-market`](docs/architecture.md#ad-market). |
| **`metrics-aggregator/` + `monitoring/`** | Dashboard and monitoring - shows you what's happening on the network in real-time | Aggregates `/metrics`, exposes `/wrappers`, `/treasury`, `/governance`, `/probe`, `/chaos`, `/remediation/*`, and feeds Grafana dashboards. |
| **Tooling** (`scripts/`, `tools/`, `examples/`, `sim/`, `fuzz/`, `formal/`) | Scripts and tests to make sure everything works correctly | Bootstrap scripts, dependency policy tooling, settlement/replay harnesses, chaos testing, fuzzing, and formal verification inputs. |

### Recent Major Additions
- **Receipt Integration System (December 2025)**: Consensus-level audit trail for all market settlements
  - Receipts (Storage, Compute, Energy, Ad) now included in block hash for consensus validation
  - Telemetry system tracks receipt emission by market type (`receipts_storage_total`, `receipts_per_block`, etc.)
  - Deterministic metrics engine derives market utilization from on-chain receipt data
  - Launch Governor can now use real market activity (not placeholders) for economic gates
  - See `RECEIPT_INTEGRATION_INDEX.md` for complete documentation
- **Treasury Disbursement System**: Complete end-to-end workflow for governance-approved fund distributions with RPC handlers (`gov.treasury.submit_disbursement`, `execute_disbursement`, `rollback_disbursement`) and full validation
- **Disbursement Status Machine**: Queue/timelock/rollback logic now flows through `gov.treasury.queue_disbursement` (driven from `contract-cli gov disburse queue`, which auto-derives the current epoch), and metrics/explorer surfaces now emit the full Draft → Voting → Queued → Timelocked → Executed/Finalized/RolledBack labels so operators can see exactly where each payout sits before execution
- **Energy Market Signature Verification**: Trait-based multi-provider signature system with Ed25519 (always available) and Dilithium (post-quantum, feature-gated), enabling oracle meter readings to be cryptographically verified
- **Comprehensive Testing**: 100+ new unit tests covering signature verification, credit persistence across provider restarts, oracle timeout enforcement, and disbursement validation

See [`docs/overview.md`](docs/overview.md#document-map) for the authoritative document map.

---

## 5-Minute Local Demo (Try It Now)

Want to see The Block running on your machine? Here's the fastest path:

```bash
# 1. Bootstrap (installs Rust, Python venv, etc.)
./scripts/bootstrap.sh

# 2. Build the node and CLI
cargo build -p the_block --release
cargo build -p contract-cli --bin contract-cli

# 3. Start a local node
./target/release/contract-cli node start --config config/node.toml

# 4. In another terminal, try some commands:
./target/release/contract-cli wallet new              # Create a wallet
./target/release/contract-cli explorer blocks --tail 5   # See recent blocks
./target/release/contract-cli tx send --help          # Explore sending BLOCK
```

That's it! You're running a local blockchain. See [`docs/operations.md`](docs/operations.md) for production deployment.

> **Homebrew updates on macOS:** `scripts/bootstrap.sh` now exports `HOMEBREW_NO_AUTO_UPDATE=1` before touching Homebrew so it never rewrites every cask when you run it. Run `brew update` manually before the bootstrap if you need newer packages (or unset the variable before the script: `HOMEBREW_NO_AUTO_UPDATE=0 ./scripts/bootstrap.sh`). The script also downloads the macOS `cargo-make`/`cargo-nextest` assets (`x86_64-apple-darwin` and `aarch64-apple-darwin`), so rerunning it on Apple Silicon should no longer end with `unsupported architecture: arm64-Darwin`.

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
   contract-cli node start --config node/.env.example
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
| [`docs/architecture.md#launch-governor`](docs/architecture.md#launch-governor) | Launch autopilot, readiness gates, decision snapshots, and configuration. |
| [`docs/economics_and_governance.md`](docs/economics_and_governance.md) | BLOCK supply, fee lanes, subsidy multipliers, treasury, governance DAG, settlement math; older docs may reference legacy `_ct` labels for the same BLOCK flows. |
| [`docs/operations.md`](docs/operations.md) | Bootstrap, configuration, telemetry wiring, runbooks, probe/diagnostics, WAL/snapshot care, deployments. |
| [`docs/security_and_privacy.md`](docs/security_and_privacy.md) | Threat model, crypto stack, remote signers, jurisdiction packs, LE portal, supply-chain security. |
| [`docs/developer_handbook.md`](docs/developer_handbook.md) | Environment setup, coding standards, testing/fuzzing, simulation, dependency policy, WASM/contracts, contribution flow. |
| [`docs/apis_and_tooling.md`](docs/apis_and_tooling.md) | JSON-RPC, CLI, gateway HTTP & DNS, explorer, light-client streaming, storage APIs, probe CLI, metrics schemas. |

Everything is kept in sync with `mdbook`; CI blocks merges if documentation drifts from the implementation.

> **Note:** The canonical currency name is **BLOCK** and every user-facing surface should use BLOCK, but the ledger still exposes legacy `_ct` gauge/field names (e.g., `STORAGE_SUB_CT`). Treat those identifiers as BLOCK-denominated ledgers until all telemetry is migrated.

---

## Contributing & support
- **New contributors:** read [`AGENTS.md`](AGENTS.md) once and work like you authored it. It documents expectations, testing gates, release policy, escalation paths, and the Document Map owners you must loop in before touching each subsystem. The spec-first contract applies even to README/handbook updates—patch docs first, cite them in your PR, then change code.
- **Ad-market track:** With the cohort/presence spec locked in this README + `docs/`, implementation now moves to the backlog in `AGENTS.md §15.K`. Code/CLI/RPC changes must cite the doc sections updated here and include the readiness checklist artifacts (`docs/overview.md#ad--targeting-readiness-checklist`).
- **Repeatable workflows:** prefer `just` targets and the scripts under `scripts/` instead of ad-hoc commands. Hook `scripts/pre-commit.sample` if you want automatic fmt/lint before each commit.
- **Issues/questions:** open an issue describing the doc/code mismatch before changing behavior—spec drift is treated as a bug.
- **Licensing:** Apache 2.0 (`LICENSE`) governs the entire repo, including generated artifacts.

When in doubt, update the docs first, then patch the code. Production readiness is assumed at every commit—tests, reproducible builds, and telemetry are not optional.

---

## New to Blockchains? Start Here

If you're completely new to blockchains, here's the recommended reading path:

1. **This README** — You're here! Get the big picture.
2. **[`docs/overview.md`](docs/overview.md)** — Learn what lives where in the codebase.
3. **[`docs/architecture.md`](docs/architecture.md)** — Start with "Ledger and Consensus" and "Transaction and Execution Pipeline" sections.
4. **[`docs/economics_and_governance.md`](docs/economics_and_governance.md)** — Understand how CT flows and how decisions get made.
5. **[`docs/developer_handbook.md#environment-setup`](docs/developer_handbook.md#environment-setup)** — Set up your dev environment.
6. **[`AGENTS.md`](AGENTS.md)** — The contributor bible. Read once, work like you wrote it.

**Key concepts you'll encounter:**
| Term | Plain English |
|------|---------------|
| Block | A page in the shared ledger (added every ~1 second) |
| Consensus | How nodes agree which page is next |
| BLOCK | The single token (40M max supply) — like money for the network |
| Mempool | The waiting room for transactions before they're added to a block |
| Macro-block | A periodic checkpoint that makes syncing faster |
| SNARK | A small cryptographic proof that heavy computation was done correctly |
| Treasury | The community fund; disbursements require governance votes |
