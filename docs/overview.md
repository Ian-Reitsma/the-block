# Overview

> **If You're Brand New to Blockchains**
>
> Before diving in, here are three concepts you'll see everywhere:
>
> | Concept | Plain English |
> |---------|---------------|
> | **Block** | A page in a shared ledger. Every ~1 second, a new page is added containing recent transactions. Once added, it can't be changed. |
> | **Consensus** | How nodes (computers running the software) agree on which page is next. This prevents anyone from cheating or rewriting history. |
> | **BLOCK** | The single currency that moves through the system. You can send it, receive it, pay for services, or earn it by contributing. |
>
> **Reading this doc:** If you want to know *what lives where* in the codebase, read this. If you want *wire-level technical details*, read [`docs/architecture.md`](architecture.md).

The Block is the unification layer for storage, compute, networking, and governance that turns verifiable work into BLOCK rewards. Everything in the workspace is owned by the maintainers—no third-party stacks in consensus or networking—so the documentation describes what already ships in `main`, not a roadmap.

> **Legacy labels:** Internally we still refer to metrics like `STORAGE_SUB_CT` even though the transferred token is BLOCK. Treat any `_ct` identifiers you see in gauges or proto files as BLOCK-denominated ledgers until those names are retired.

## Mission
- Operate a one-second base layer that notarizes micro-shard roots while keeping the L1 deterministic and audit-friendly.
- Pay operators for real service (`STORAGE_SUB_CT`, `READ_SUB_CT`, `COMPUTE_SUB_CT`) instead of speculative gas schedules.
- Treat governance as an engineering surface: the same crate powers the node, CLI, explorer, and telemetry so proposals, fee-floor policies, and service-badge status never drift.
- Ship first-party clients: the in-house HTTP/TLS stack (`crates/httpd` + `crates/transport`) fronts every RPC, gateway, and gossip surface, and dependency pivots move through governance before they land in production.
- Automate readiness via the Launch Governor: a streak-based autopilot (`node/src/launch_governor`) watches chain/DNS (and soon economics/market) telemetry, records signed decisions, and flips runtime gates so testnet/mainnet transitions stay auditable.

## Responsibility Domains

| Domain | What It Does (Plain English) | Repository roots | In-flight scope |
| --- | --- | --- | --- |
| **Consensus & Ledger** | Decides which transactions "really happened" and in what order. Makes sure everyone agrees on the same history. | `node/src/consensus`, `node/src/blockchain`, `bridges`, `ledger`, `poh` | Hybrid PoW/PoS leader schedule, macro-block checkpoints, Kalman retarget, ledger invariants, bridge proofs. |
| **Serialization & Tooling** | Agrees on a specific binary format so nodes written in different languages still understand each other. | `crates/foundation_serialization`, `crates/codec`, `docs/spec/*.json` | Canonical binary layout, cross-language vectors, CLI/SDK adapters. |
| **Cryptography & Identity** | Makes sure signatures can't be faked and identities can be updated or revoked. Handles keys and proofs. | `crypto`, `crates/crypto_suite`, `node/src/identity`, `dkg`, `zkp`, `remote_signer` | Hash/signature primitives, DKG, commit–reveal, identity registries, PQ hooks. |
| **Core Tooling & UX** | The command-line app (`contract-cli`), dashboards, and explorer that people actually touch. | `cli`, `gateway`, `explorer`, `metrics-aggregator`, `monitoring`, `docs/apis_and_tooling.md` | RPC & CLI surfaces, gateways, dashboards, probe CLI, release tooling. |
| **Launch Governor & Autopilot** | Automates readiness transitions (operational, DNS, upcoming economics/market gates) and publishes signed intent snapshots, watching the `economics_epoch_*` gauges plus `economics_block_reward_per_block` before promoting reward gates. | `node/src/launch_governor`, `node/src/governor_snapshot.rs`, `docs/architecture.md#launch-governor`, `docs/operations.md#launch-governor-operations` | Streak-tuned gating, decision signing, economics/market gates in backlog. |

## Design Pillars
| Pillar | Enforcement | Evidence |
| --- | --- | --- |
| Determinism | `#![forbid(unsafe_code)]`, `codec::profiles`, ledger replay tests cross `x86_64`/`aarch64`. | `cargo test -p the_block --test replay` and mdBook specs under `docs/architecture.md`. |
| Memory & Thread Safety | First-party runtime, no `unsafe`, concurrency helpers in `crates/concurrency`. | `miri`/ASan gates in CI, locking helpers (`MutexExt`, `DashMap`) wrap every shared structure. |
| Portability | Build matrix (Linux glibc/musl, macOS, Windows/WSL) plus `scripts/bootstrap.*`. | `Justfile` + `Makefile` run the same steps locally and in CI; provenance signatures gate releases. |

## End-to-End Flow

**Story Example: Alice uploads a file**

Imagine Alice wants to store a file on The Block. Here's what happens step by step:

1. Alice's wallet app sends a "store this file" request to a gateway node
2. The gateway encrypts the file and creates a transaction
3. The transaction enters the mempool (waiting room)
4. A miner includes it in the next block
5. The block is validated and propagated to all nodes
6. Alice's CT balance decreases; the storage provider's balance increases
7. Alice can now see the transaction in the explorer or CLI

**Technical Breakdown:**

| Step | What Happens | Code Module |
|------|--------------|-------------|
| 1. **Ingress** | Gateways accept blobs and RPCs over the in-house `httpd` router, encrypt/store via `node/src/storage` and `storage_market` receipts, and emit signed `ReadAck` acknowledgements. | `node/src/gateway`, `gateway/` |
| 2. **Mempool** | Transaction enters the waiting room where fee-floor policy determines priority. | `node/src/mempool` |
| 3. **Scheduling** | The multi-lane scheduler batches consumer/industrial traffic, applies fee-floor policy, and records QoS counters. | `node/src/scheduler.rs` |
| 4. **Consensus** | The hybrid PoW/PoS engine enforces macro-block checkpoints, PoH ticks, VDF randomness, and difficulty retune while gossip/range-boost propagate blocks. | `node/src/consensus` |
| 5. **Rewarding** | Subsidy accounting, service-badge tracking, treasury streaming, and governance DAG state are updated; CT moves between accounts. | `node/src/governance`, `governance/` |
| 6. **Observability** | Runtime telemetry, metrics aggregator, and dashboards reflect the new state. | `node/src/telemetry.rs`, `metrics-aggregator/`, `monitoring/` |

## Repository Layout (live tree)
| Path | Highlights |
| --- | --- |
| `node/` | Full node, gateway stack, compute/storage/bridge/mempool modules, RPC server. |
| `crates/` | First-party libraries: transport, HTTP, serialization, overlay, runtime, coding/erasure, wallet SDKs. |
| `cli/` | `contract-cli` binary with governance, bridge, wallet, identity, compute, telemetry, and remediation commands. |
| `metrics-aggregator/` | Aggregates Prometheus-style metrics, publishes dashboards, verifies TLS & governance state. |
| `monitoring/` | Grafana/Prometheus templates and scripts (build via `npm ci --prefix monitoring`). |
| `storage_market/`, `dex/`, `bridges/`, `gateway/` | Dedicated crates for specialized subsystems referenced throughout the docs. |
| `docs/` | The consolidated handbook you are reading (mdBook enabled). |

## Energy Market Snapshot

**How It Feels to a Human:**

As an energy provider, you register once, then your smart meter sends signed readings to the network. These readings turn into "credits" (promises of energy). When customers use your energy, those credits become "receipts" (proof of delivery). If someone says "this reading looks wrong," they can file a dispute — a special record that triggers review.

**Example Flow:**
1. Provider registers with capacity: 10,000 kWh, price: 50 CT per kWh
2. Smart meter reading: 1,000 kWh delivered → creates an `EnergyCredit`
3. Customer settles 500 kWh → creates an `EnergyReceipt` (with treasury fee deducted)
4. Provider receives CT in their account

**Technical Details:**
- **Code surface** — `crates/energy-market` implements providers, credits, receipts, and telemetry; `node/src/energy.rs` persists them in sled (`SimpleDb::open_named(names::ENERGY_MARKET, …)`), applies governance hooks, and exposes health checks. RPC handlers live in `node/src/rpc/energy.rs`, the CLI entry point is `cli/src/energy.rs`, and oracle ingestion goes through `crates/oracle-adapter` plus the `services/mock-energy-oracle` binary used by the World OS drill.
- **State & persistence** — Energy state is serialized with `foundation_serialization::binary::{encode,decode}` and stored wherever `TB_ENERGY_MARKET_DIR` points (default `energy_market/`). Snapshots occur after every mutation, mirroring the fsync+rename workflow the rest of `SimpleDb` uses so restarts replay identical providers/credits/receipts. Governance parameters (`energy_min_stake`, `energy_oracle_timeout_blocks`, `energy_slashing_rate_bps`) share the same proposal pipeline as other params; once a proposal activates, `node::energy::set_governance_params` updates the runtime config and re-snapshots the sled DB.
- **RPC & CLI** — The JSON-RPC namespace exposes `energy.register_provider`, `energy.market_state`, `energy.submit_reading`, and `energy.settle`. Requests use the exact schema documented in `docs/apis_and_tooling.md#energy-rpc-payloads-auth-and-error-contracts`, including the shared `MeterReadingPayload` used by oracle adapters, CLI tooling, and explorers. `contract-cli energy` prints tabular output by default, toggles JSON via `--verbose`/`--format json`, and pipes raw payloads to automation without diverging from the node schema.
- **Observability & operations** — Runtime metrics include gauges (`energy_providers_count`, `energy_avg_price`), counters (`energy_kwh_traded_total`, `energy_settlements_total{provider}`, `energy_signature_failure_total{provider,reason}`), and histograms (`energy_provider_fulfillment_ms`, `oracle_reading_latency_seconds`). `node::energy::check_energy_market_health` logs warnings when pending credits pile up or settlements stall. `docs/testnet/ENERGY_QUICKSTART.md` plus `scripts/deploy-worldos-testnet.sh` describe the canonical bootstrap procedure (node + mock oracle + telemetry stack); `docs/operations.md#energy-market-operations` extends the runbook with backup, dispute, and alerting guidance.
- **Security & governance alignment** — The outstanding work (oracle signature enforcement, dispute RPCs, explorer timelines, QUIC chaos drills, sled snapshot drills, release-provenance gates) is tracked in `docs/architecture.md#energy-governance-and-rpc-next-tasks` and summarized in `AGENTS.md`. `docs/security_and_privacy.md#energy-oracle-safety` documents key hygiene, secret sourcing, and telemetry redaction requirements for oracle adapters.

## Reference Workflow
1. Read `AGENTS.md` and this overview once—then work like you wrote them.
2. Run `scripts/bootstrap.sh` (or `.ps1`) to install Rust 1.86+, `cargo-nextest`, Python 3.12.3 venv, and toolchain shims.
3. Use `just lint`, `just fmt`, `just test-fast`, and `just test-full` to stay in sync with CI.
4. Keep dependency policy artifacts (`docs/dependency_inventory*.json`) up to date via `cargo run -p dependency_registry` or `just dependency-audit`.
5. Wire telemetry locally: `metrics-aggregator`, `monitoring/`, and `crates/probe` exercise the same endpoints operators rely on.

## Quick Glossary

Terms you'll encounter in architecture docs:

| Term | Plain English | Code/Docs Reference |
|------|---------------|---------------------|
| **SNARK receipts** | Small cryptographic proofs that heavy computations were done correctly. Instead of re-running a job, validators just check the proof. | `node/src/compute_market`, [`architecture.md#compute-marketplace`](architecture.md#compute-marketplace) |
| **LocalNet** | A local mesh of nearby devices relaying data to each other. Provides instant starts and low latency for video/downloads/games. | `node/src/localnet`, [`architecture.md#localnet-and-range-boost`](architecture.md#localnet-and-range-boost) |
| **Range Boost** | Extended wireless/mesh nodes providing coverage over longer distances. Store-and-forward for rural or spotty areas. | `node/src/range_boost`, [`architecture.md#localnet-and-range-boost`](architecture.md#localnet-and-range-boost) |
| **Mobile cache** | Encrypted on-device cache for offline operation. State syncs once you're back online. | `gateway/`, `node/src/gateway` |
| **Light client** | A lightweight client that follows the chain using proofs without storing everything. Good for phones and browsers. | [`architecture.md#gateway-and-client-access`](architecture.md#gateway-and-client-access) |
| **Macro-block** | A periodic checkpoint (every N blocks) that makes syncing faster. Contains per-shard state roots. | [`architecture.md#ledger-and-consensus`](architecture.md#ledger-and-consensus) |
| **Service badge** | A status mark in the ledger showing a node has been "good enough" recently (uptime, service quality). Affects voting weight. | `node/src/service_badge.rs`, [`economics_and_governance.md`](economics_and_governance.md) |
| **Treasury disbursement** | Moving CT from the community fund to a destination. Requires governance votes and a timelock period. | `governance/src/treasury.rs`, [`economics_and_governance.md`](economics_and_governance.md) |

## Document Map
All remaining detail sits in six focused guides:
- [`docs/architecture.md`](architecture.md) — ledger, networking, storage, compute, bridges, gateway, telemetry.
- [`docs/architecture.md#launch-governor`](architecture.md#launch-governor) — autopilot design, gate signals, env vars, and RPCs.
- [`docs/economics_and_governance.md`](economics_and_governance.md) — CT supply, fees, treasury, proposals, service badges, kill switches.
- [`docs/operations.md`](operations.md) — bootstrap, deployments, telemetry wiring, dashboards, runbooks, chaos & recovery.
- [`docs/security_and_privacy.md`](security_and_privacy.md) — threat modelling, cryptography, remote signer flows, jurisdiction policy packs, LE portal, supply-chain controls.
- [`docs/developer_handbook.md`](developer_handbook.md) — environment setup, coding standards, testing/fuzzing, simulation, dependency policy, tooling.
- [`docs/apis_and_tooling.md`](apis_and_tooling.md) — JSON-RPC, CLI, gateway HTTP & DNS, explorer, probe CLI, metrics endpoints, schema references.
- [`docs/subsystem_atlas.md`](subsystem_atlas.md) — workspace atlas + node subsystem index that maps every directory/file to plain-English context so newcomers know where to start.
- **🆕 [`RECEIPT_INTEGRATION_INDEX.md`](../RECEIPT_INTEGRATION_INDEX.md) — Receipt integration guide**: Market audit trails, consensus validation, telemetry system, metrics derivation. See this for the complete receipt integration status, implementation guides, and next steps (December 2025).

For historical breadcrumbs the removed per-subsystem files now redirect through [`docs/LEGACY_MAPPING.md`](LEGACY_MAPPING.md).

### Owner routing
Tag the following owners whenever you touch the listed scope. These handles map directly to reviewer groups in GitHub/Gerrit and keep the Document Map actionable:

| Area | Owners to tag | Canonical paths | Critical dependencies |
| --- | --- | --- | --- |
| Ad + Targeting Platform | `@ad-market` (primary), `@gov-core`, `@gateway-stack`, `@telemetry-ops` | `crates/ad_market/**`, `node/src/{ad_policy_snapshot.rs,ad_readiness.rs,localnet,range_boost,gateway/dns.rs,rpc/ad_market.rs}`, `node/tests/ad_market_rpc.rs`, `cli/src/{ad_market.rs,gov.rs,explorer.rs}`, `metrics-aggregator/**`, `monitoring/**` | Range Boost + LocalNet presence proofs, `.block` DNS/auction feeds, privacy budget manager, `/wrappers` + Grafana dashboards, governance-configured selector registries |
- **Spec circulation log** — The ad-market blueprint captured in `docs/architecture.md#ad-marketplace`, `docs/system_reference.md#appendix-a-·-rpc-method-index`, `docs/operations.md#ad-market-operations`, and `docs/security_and_privacy.md#privacy-layers` was circulated to `@ad-market`, `@gov-core`, `@gateway-stack`, and `@telemetry-ops` (Document Map owners above) for ACK on 2024-05-25. Implementation owners should now unblock the code/CLI/RPC backlog in `AGENTS.md §15.K`, referencing this spec and citing doc PRs in every code review.
- **Docs parity watch** — `@docs-core` + subsystem owners must keep `README.md`, this overview, `docs/developer_handbook.md`, and `docs/apis_and_tooling.md` aligned whenever selectors, presence proofs, or telemetry knobs evolve (see `AGENTS.md §15.L`). Call out any lagging doc delta in PR descriptions until parity is restored.

## Execution Backlog & Ownership Handoff

Engineering work proceeds against the backlog enumerated in `AGENTS.md §§0.6, 15.A–15.J`. Each subsection is mapped to canonical file paths and telemetry hooks so ownership is explicit:

- **Spec & Quality guardrails** — All teams must prove spec/implementation alignment (`AGENTS.md:93-101`) before writing code. File doc diffs first, cite the Document Map owner, and attach the standard test cadence transcript (`just lint`, `just fmt`, `just test-fast`, tiered `just test-full`, `cargo test -p the_block --test replay`, settlement audit, `scripts/fuzz_coverage.sh`) to every review. Observability for new surfaces must route through `node/src/telemetry`, `metrics-aggregator/`, and `monitoring/`, with `/wrappers` documentation updates in `docs/operations.md`.
- **Governance + treasury** — Governance, ledger, CLI, explorer, and telemetry artifacts need multi-stage treasury approvals, attested release bundles, disbursement lag/failure metrics, and deterministic replay coverage. Work-in-progress lives in `governance/`, `node/src/governance`, `node/src/treasury_executor.rs`, `cli/src/governance`, `docs/economics_and_governance.md`, and `docs/operations.md`.
- **Compute-market SLA controls** — `node/src/compute_market/**`, `monitoring/`, and explorer dashboards must implement SLA slashing, fairness windows, deterministic receipt replays, and operator remediation tooling. Documentation resides in `docs/architecture.md#compute-marketplace`.
- **Networking + range-boost reliability** — `node/src/net`, `node/src/p2p`, `range_boost/`, `crates/transport`, and `metrics-aggregator/` must support WAN-scale QUIC chaos drills with runbooks documented in `docs/architecture.md#networking-and-propagation` and `docs/operations.md#bootstrap-and-configuration`.
- **Wallet/remote signer UX** — `cli/src/wallet`, `node/src/identity`, `remote_signer/`, and `tests/remote_signer_*.rs` are responsible for batched signer discovery, telemetry, and LE portal updates mirrored into `docs/security_and_privacy.md` and `docs/apis_and_tooling.md`.
- **Bridges + DEX** — `bridges/`, `dex/`, explorer timelines, and `docs/architecture.md#token-bridges`/`#dex-and-trust-lines` must cover signer-set payloads, telemetry pipelines, proofs, and release-verifier workflows documented under `docs/security_and_privacy.md#release-provenance-and-supply-chain`.
- **Storage + dependency drills** — `node/src/simple_db`, `storage/`, `storage_market/`, `coding/`, and CI harnesses inside `scripts/`/`formal/` run snapshot/restore drills, dependency fault simulations, and ledger parity checks. Procedures live in `docs/operations.md#storage-and-state`.
- **Energy governance/interfaces** — `governance/`, `node/src/energy.rs`, `cli/src/energy.rs`, `crates/energy-market`, `crates/oracle-adapter`, and `services/mock-energy-oracle` need production-grade oracle verification, governance payloads (batch vs real-time), schema-aligned CLI workflows, telemetry dashboards, CI gates, and runbooks per `docs/architecture.md#energy-governance-and-rpc-next-tasks`, `docs/economics_and_governance.md`, `docs/operations.md#energy-market-operations`, `docs/security_and_privacy.md#energy-oracle-safety`, and `docs/testnet/ENERGY_QUICKSTART.md`.
- **Docs & onboarding parity** — Track outstanding README/overview/handbook/API diffs inside `AGENTS.md §15.L` and close them in the same PRs that modify code. mdBook (`docs/`) is the spec: run `mdbook build docs` before submitting doc changes and reject reviews that change behaviour without touching the docs listed above.

Every TODO added to the codebase must be mirrored into `AGENTS.md §15` with a pointer back to the files above so the backlog remains synchronized across documentation, telemetry, and implementation.

### Ad + Targeting readiness checklist
- **When to run** — Any PR that touches `crates/ad_market`, `node/src/rpc/ad_market.rs`, `node/src/{ad_policy_snapshot.rs,ad_readiness.rs,localnet,range_boost,gateway/dns.rs,read_receipt.rs,service_badge.rs}`, `cli/src/{ad_market.rs,gov.rs,explorer.rs}`, `metrics-aggregator/**`, or `monitoring/**`.
- **Commands (attach transcripts/CI links)** — `just lint`, `just fmt`, `just test-fast`, `just test-full`, `cargo test -p the_block --test replay`, `cargo test -p the_block --test settlement_audit --release`, and `scripts/fuzz_coverage.sh`. Record the fuzz `.profraw` summary alongside the console logs so reviewers can verify the guardrail cited in `AGENTS.md §0.6`.
- **Telemetry proof** — When selector, privacy, or readiness metrics change, run `npm ci --prefix monitoring && make monitor`, capture the before/after Grafana JSON diff or screenshot for the `Ad Market Readiness` dashboard, and note the refreshed `/wrappers` payload hash exported by `metrics-aggregator`.
- **Skipped steps** — If an owner-approved skip is required (for example, `just test-full` unhealthy in CI), document the approver and reason directly in the PR description and mirror it in the worklog so governance reviewers can see the exception trail.
