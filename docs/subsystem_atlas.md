# Subsystem Atlas

This atlas supplements `AGENTS.md`, `docs/overview.md`, and the subsystem specs by mapping every major path in the repository to plain-language descriptions. Treat it as the first stop for new contributors (or agents) who need to understand where a concept lives without digging through source code. Each entry lists the canonical files, the kind of work that happens there, and the doc sections that provide deeper context.

## Common Tasks → Paths

When you want to change something specific, start here:

| Task | Code Paths | Doc References |
|------|------------|----------------|
| **Change how transaction fees work** | `node/src/fee`, `governance/src/params.rs`, `cli/src/fee_estimator.rs` | [`economics_and_governance.md#fee-lanes-and-rebates`](economics_and_governance.md#fee-lanes-and-rebates) |
| **Economic autopilot** | `node/src/lib.rs`, `node/src/economics/mod.rs`, `node/src/telemetry.rs` | `NetworkIssuanceController` uses `economics_epoch_tx_count`, `economics_epoch_tx_volume_block`, `economics_epoch_treasury_inflow_block`, and `economics_block_reward_per_block` to align block rewards before Launch Governor flips gates (`docs/economics_and_governance.md#network-driven-block-issuance`). |
| **Add or tune a governance parameter** | `governance/src/params.rs`, `node/src/governance/params.rs`, `cli/src/gov.rs` | [`economics_and_governance.md#governance-parameters`](economics_and_governance.md#governance-parameters) |
| **Integrate a new ad-market data source** | `crates/ad_market`, `node/src/ad_policy_snapshot.rs`, `node/src/ad_readiness.rs` | [`architecture.md#ad-market`](architecture.md#ad-market) |
| **Add a new RPC endpoint** | `node/src/rpc/*.rs`, `cli/src/*.rs` | [`apis_and_tooling.md#json-rpc`](apis_and_tooling.md#json-rpc) |
| **Modify energy market behavior** | `crates/energy-market`, `node/src/energy.rs`, `node/src/rpc/energy.rs`, `cli/src/energy.rs` | [`architecture.md#energy-governance-and-rpc-next-tasks`](architecture.md#energy-governance-and-rpc-next-tasks) |
| **Change compute-market pricing/SLA** | `node/src/compute_market/settlement.rs`, `governance/src/params.rs`, `monitoring/` dashboards | [`architecture.md#compute-marketplace`](architecture.md#compute-marketplace) |
| **Add telemetry for a new metric** | `node/src/telemetry.rs`, `metrics-aggregator/`, `monitoring/` | [`architecture.md#telemetry-and-instrumentation`](architecture.md#telemetry-and-instrumentation) |
| **Modify consensus rules** | `node/src/consensus`, `node/src/blockchain` | [`architecture.md#ledger-and-consensus`](architecture.md#ledger-and-consensus) |
| **Update storage pipeline** | `node/src/storage/pipeline.rs`, `coding/`, `storage_market/` | [`architecture.md#storage-and-state`](architecture.md#storage-and-state) |
| **Add a new CLI command** | `cli/src/*.rs`, register in `cli/src/main.rs` | [`apis_and_tooling.md#cli`](apis_and_tooling.md#cli-contract-cli) |
| **Configure or extend Launch Governor gates** | `node/src/launch_governor/mod.rs`, `node/src/governor_snapshot.rs`, `node/src/rpc/governor.rs` | [`architecture.md#launch-governor`](architecture.md#launch-governor), [`operations.md#launch-governor-operations`](operations.md#launch-governor-operations) |

## Workspace Atlas

### Core Infrastructure

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `node/` | Full node implementation | `src/consensus`, `src/blockchain`, `src/mempool`, `src/rpc` |
| `crates/` | Shared libraries | `foundation_*`, `transport`, `httpd`, `storage_engine`, `p2p_overlay`, `wallet` |
| `cli/` | Command-line interface | `src/main.rs`, `src/gov.rs`, `src/wallet.rs`, `src/energy.rs` |
| `governance/` | Governance logic | `src/params.rs`, `src/treasury.rs`, `src/bicameral.rs`, `src/proposals.rs` |

### Markets

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `crates/energy-market/` | Energy trading marketplace | Types, settlement, oracle integration |
| `node/src/compute_market/` | Compute job marketplace | `scheduler.rs`, `settlement.rs`, `snark.rs` |
| `crates/ad_market/` | Privacy-aware ad system | `badge.rs`, `budget.rs`, `privacy.rs`, `uplift.rs` |
| `storage_market/` | Decentralized storage | Rent, providers, redundancy |
| `dex/` | Decentralized exchange | Order books, trust lines, HTLCs |

### Networking & Transport

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `crates/transport/` | QUIC transport abstraction | Quinn, s2n providers |
| `node/src/p2p/` | Peer-to-peer networking | `handshake.rs`, `wire_binary.rs` |
| `node/src/gossip/` | Gossip protocol | `relay.rs` |
| `node/src/localnet/` | Device-to-device mesh | Proximity proofs |
| `node/src/range_boost/` | Extended coverage | Queue, forwarder |

### Gateway & Clients

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `gateway/` | HTTP gateway stack | Mobile cache, DNS publishing |
| `node/src/gateway/` | Gateway integration | `http.rs`, `dns.rs`, `mobile_cache.rs`, `read_receipt.rs` |
| `node/src/light_client/` | Lightweight sync | Header streaming, proofs |

### Infrastructure

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `bridges/` | Cross-chain bridges | `light_client.rs`, `relayer.rs`, `token_bridge.rs` |
| `metrics-aggregator/` | Metrics collection | Dashboard endpoints |
| `monitoring/` | Grafana/Prometheus | JSON dashboards |
| `explorer/` | Block explorer | Web UI, APIs |

### Security & Identity

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `crypto/` | Cryptographic primitives | Hashes, signatures |
| `crates/crypto_suite/` | Crypto abstractions | Ed25519, Dilithium, Kyber |
| `node/src/identity/` | DIDs and handles | `handle_registry.rs` |
| `dkg/` | Distributed key generation | Committee keys |
| `zkp/` | Zero-knowledge proofs | SNARK verification |
| `privacy/` | Privacy utilities | Read receipt privacy |

## Legacy Name Mapping

If you see old names in code, here's what they mean now:

| Old Name | Current Name | Notes |
|----------|--------------|-------|
| `amount_it` | (Removed) | Legacy disbursement field replaced by single `amount` |
| `payout_it` | (Removed) | Legacy industrial payout label |
| Various `docs/*.md` that no longer exist | See [`LEGACY_MAPPING.md`](LEGACY_MAPPING.md) | Consolidated into main docs |

See [`LEGACY_MAPPING.md`](LEGACY_MAPPING.md) for the full historical mapping.
The remaining directories (`crypto/`, `inflation/`, `privacy/`, `services/`, `examples/`, `explorer/`, etc.) each host standalone binaries, proofs, or sample artifacts. Use `rg --files` plus this atlas to jump into the relevant code when the doc map alone is not enough.

The `node/` crate is densely packed. This index spells out every module so that even contributors with zero blockchain experience can map functionality to files.

### Ledger, Blocks, and Serialization

| Path | Description |
| --- | --- |
| `node/src/blockchain/` | Core ledger state machine. Handles block application, forks, replay helpers, genesis wiring. |
| `node/src/block_binary.rs`, `node/src/ledger_binary.rs`, `node/src/legacy_cbor.rs` | Canonical serialization profiles (current binary, legacy CBOR) plus helpers used by explorers/tests. |
| `node/src/hash_genesis.rs`, `node/src/hashlayout.rs`, `node/src/blob_chain.rs` | Genesis hash seeds, Merkle layout definitions, blob-chain glue code for storage-backed ledger data. |
| `node/src/ledger_binary.rs` | Binary ledger snapshots and deterministic serialization for audit tooling. |
| `node/src/update.rs` | Handles self-upgrade metadata, release channels, and hotfix tracking. |

### Consensus, PoH, and Scheduling

| Path | Description |
| --- | --- |
| `node/src/consensus/` | Hybrid PoW/PoS engine with macro-block checkpoints, fork choice, and Kalman difficulty retune. |
| `node/src/poh.rs` | Proof-of-History tick generator feeding the consensus engine. |
| `node/src/parallel.rs` | Conflict-aware executor used by the scheduler to run non-overlapping tasks in parallel. For newcomers: each `Task` declares read/write keys; the executor groups non-conflicting tasks and runs them on scoped threads while exporting telemetry (`PARALLEL_EXECUTE_SECONDS`). |
| `node/src/scheduler.rs` | Multi-lane scheduler that batches consumer, industrial, and compute workloads. Integrates with `compute_market/` fairness windows and exposes QoS metrics. |
| `node/src/partition_recover.rs` | Replay helper that re-validates blocks after a network partition heals. Uses `validate_and_apply` plus `ExecutionContext` to keep the ledger deterministic and increments `PARTITION_RECOVER_BLOCKS` telemetry counters. |
| `node/src/poh.rs`, `node/src/constants.rs` | Timing constants and tick generators referenced across consensus, gossip, and range-boost code. |

### Transactions, Fees, and Accounts

| Path | Description |
| --- | --- |
| `node/src/transaction.rs`, `tx/`, `transaction/` | Transaction structs, signatures, serialization formats, and CLI-friendly helpers. |
| `node/src/fee/`, `node/src/fees.rs`, `node/src/fees/` (including `lane_pricing.rs`, `congestion.rs`, `market_signals.rs`) | Fee-floor enforcement, lane pricing engine, congestion telemetry, and QoS logic used by mempool + scheduler. |
| `node/src/accounts/` | Session policies and pluggable account validation (`AccountValidation`, `SessionPolicy`). Useful when building wallet abstractions or remote signers. |
| `node/src/mempool/` | Admission queues, gossip integration, QoS counters, admissions policies. |
| `node/src/utxo/`, `node/src/liquidity/` | UTXO tracking and liquidity routing helpers for DEX/treasury integrations. |

### Compute, Storage, and Marketplaces

| Path | Description |
| --- | --- |
| `node/src/compute_market/` | Offers, matcher, courier, settlement, SNARK proving, SLA slashing plumbing. Lane health telemetry and slash receipts live here. |
| `node/src/storage/`, `storage_market/`, `state/` | Blob storage pipeline, erasure coding, proofs-of-retrievability, rent accounting, sled snapshots. |
| `node/src/simple_db/` | SimpleDb snapshot layer used across subsystems (energy market, storage, governance). Implements fsync + atomic rename semantics and cross-platform safeguards. |
| `node/src/treasury_executor.rs` | BLOCK ledger hooks that convert compute/storage receipts and treasury disbursements into coinbase outputs. |
| `node/src/blob_chain.rs`, `node/src/storage/` | Glue between on-chain blob commitments and the actual storage backends. |

### Energy Market

| Path | Description |
| --- | --- |
| `node/src/energy.rs` | Wraps `crates/energy-market`: provider registry, credit persistence, settlement logic, governance hooks, and health checks. |
| `node/src/rpc/energy.rs`, `cli/src/energy.rs` | JSON-RPC handlers plus CLI surfaces for registering providers, submitting readings, and settling receipts. |
| `services/mock-energy-oracle/` | Dev/testnet oracle shim for World OS drills. |

### Governance, Treasury, and Badges

| Path | Description |
| --- | --- |
| `node/src/governance/`, `governance/` | Canonical governance crate, DAG store, bicameral voting, proposal lifecycles. |
| `node/src/governor_snapshot.rs`, `node/src/launch_governor/` | Snapshot tooling for live governance state, bootstrap helpers for testnets. |
| `node/src/treasury_executor.rs` | Multi-stage disbursement executors, attested release flow, kill-switch integration. |
| `node/src/service_badge.rs` | Badge issuance/revocation logic, uptime tracking, telemetry. |
| `node/src/ad_policy_snapshot.rs`, `node/src/ad_readiness.rs` | Ad marketplace policy + readiness snapshots with reversible migrations between `cohort_v1` tuple keys and `CohortKeyV2` (domain tier + interest + presence). Signed policies live here and feed RPC/explorer verifiers. |
| `node/src/le_portal.rs` | Law-enforcement portal logging, warrant canaries, evidence store. |

### Ad Market & Targeting

| Path | Description |
| --- | --- |
| `crates/ad_market/` | Cohort schema (`CohortKeyV2`), privacy budget manager, budget broker, uplift estimator, badge guards, and presence attestation verifier. Hosts migrations, selector validation, and governance-configured registries (interest tags, domain tiers, presence knobs). |
| `node/src/rpc/ad_market.rs`, `cli/src/ad_market.rs` | RPC + CLI entry points for selector-aware inventory, campaign registration, readiness, conversion reporting, presence cohort discovery/reservation, and privacy guardrail errors. |
| `cli/src/gov.rs`, `cli/src/explorer.rs` | Governance CLI controls for ad-market knobs (presence TTL, selector caps, privacy budgets) and explorer summaries that break out BLOCK revenue per selector/domain tier/presence bucket. The explorer commands dump per-selector payouts so dashboards/CSV exports never miss new signals. |
| `metrics-aggregator/src/lib.rs` (`ad_*` block), `monitoring/ad_market_dashboard.json` | Aggregates segment readiness counters, auction competitiveness histograms, privacy budget gauges, conversion totals, and publishes them over `/wrappers`. Keep Grafana JSON + screenshots in sync whenever selectors change (see `docs/operations.md#ad-market-operations`). |
| `node/tests/ad_market_rpc.rs` | Integration coverage for badge committees, selectors, presence attestations, conversion auth, and telemetry exports. Extend when adding selectors or RPCs. |
| `node/src/ad_policy_snapshot.rs`, `node/src/ad_readiness.rs` | Ad marketplace policy + readiness snapshots with reversible migrations between `cohort_v1` tuple keys and `CohortKeyV2` (domain tier + interest + presence). Signed policies live here and feed RPC/explorer verifiers. |

### Networking, Overlay, and Range Boost

| Path | Description |
| --- | --- |
| `node/src/net/`, `node/src/gossip/`, `node/src/p2p/` | TCP/UDP/QUIC reactors, peer stores, gossip propagation, capability negotiation. |
| `node/src/range_boost/`, `node/src/localnet/` | Range-boost mesh (proximity relays) + LocalNet overlays that mint presence receipts (`PresenceReceipt`/`PresenceBucket`) for the ad market, enforce TTL scheduling, and power partition drills. |
| `node/src/gateway/` | HTTP ingress, DNS publisher, mobile cache, read receipt batching, gateway policy enforcement. |
| `node/src/read_receipt.rs` | Signed acknowledgement batching plus presence-specific fields (`presence_badge`, `venue_id`, `crowd_size_hint`, mesh/geo contexts) feeding ad readiness proofs. |
| `node/src/http_client.rs` | First-party HTTP client used by node, CLI, and services (no third-party stacks). |
| `node/src/log_indexer.rs` | Structured log exporter feeding explorers/CLI/telemetry dashboards. |

### Security, Identity, and Remote Signers

| Path | Description |
| --- | --- |
| `node/src/identity/`, `node/src/kyc.rs` | DID registries, KYC policy hooks, jurisdiction enforcement. |
| `node/src/commit_reveal.rs` | Commit–reveal scheme shared by governance, bridge proofs, and treasury releases. |
| `node/src/dkg.rs` | Distributed key generation for committees, bridges, and badge issuers. |
| `node/src/service_badge.rs`, `node/src/le_portal.rs` | Telemetry + logging for badges and law-enforcement portals. |

### Telemetry, RPC, and Tooling

| Path | Description |
| --- | --- |
| `node/src/rpc/` | JSON-RPC namespaces (`energy.*`, `governance.*`, `compute.*`, `node.*`, etc.), rate-limit enforcement, auth middleware. |
| `node/src/telemetry/`, `node/src/telemetry.rs` | Metric definitions, sampling helpers, `/metrics` endpoint wiring. |
| `node/src/logging.rs`, `node/src/util/`, `node/src/http_client.rs` | Logging facades, utility helpers, HTTP/TLS wrappers. |
| `node/src/bin/` | Node entry points, CLI-compatible binaries, test harness executables. |
| `node/src/py.rs` | PyO3 bindings for deterministic replay + Python demos. |
| `node/src/web/` | HTTP handlers for the embedded admin panel and test UI endpoints. |

### Recovery, Provenance, and Miscellaneous

| Path | Description |
| --- | --- |
| `node/src/provenance.rs` | Build provenance attestations, dependency hash enforcement, release gating. |
| `node/src/partition_recover.rs` | Block replay helper after partitions (also referenced above under consensus). |
| `node/src/log_indexer.rs` | Indexes structured logs for explorers/CLI queries. |
| `node/src/simple_db/` | Cross-platform snapshot layer powering energy storage, governance, and more. |
| `node/src/util/`, `node/src/constants.rs` | Misc helpers, constants, and shared utilities across modules. |

## How to Use This Atlas

1. **Find the path** that matches the area you need to change. If you bump into an unfamiliar file name, use `rg --files` with that name and locate its entry here.
2. **Jump to the linked doc section** for deeper requirements and telemetry expectations. For example, `node/src/parallel.rs` references the compute marketplace and telemetry obligations documented in `docs/architecture.md#compute-marketplace`.
3. **Record TODOs** in `AGENTS.md §15` whenever you discover missing documentation or future work; mirror your code comments so the backlog remains transparent.
4. **Keep this atlas current**—if you add a new module, update this file (and link it from the Document Map) so the next contributor has instant context.
>>>>>>> a47e24783b578beb29ca36d4c577cdedbd77c0a8
