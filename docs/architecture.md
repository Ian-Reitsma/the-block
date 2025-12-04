# Architecture

Everything below reflects what ships in `main` today. Paths reference the exact modules so engineers can cross-check behaviour while hacking.

## Ledger and Consensus
### Block Format and State
- `node/src/blockchain` and `node/src/ledger_binary.rs` define the canonical block/ledger codecs using `codec::profiles`. Ledger snapshots embed service-badge flags, governance params, subsidy buckets, and AI-diagnostics toggles so upgrades round-trip without drift.
- Macro-block checkpoints (`node/src/macro_block.rs`) record per-shard state roots and finalize batches of 1-second blocks for light clients and replay harnesses.
- Genesis material stays in `hash_genesis.rs`; the compile-time assertion in `node/src/consensus/mod.rs` panics if `GENESIS_HASH` drifts from the serialized baseline.
- Blob chain and root assembly live in `node/src/blob_chain.rs`; roots are scheduled deterministically alongside block production.

### Serialization & Codecs
- Canonical codecs are implemented via the `foundation_serialization` facade and the `codec` crate. Binary layouts used by the node, CLI, explorer, and metrics aggregator round-trip under these profiles.
- JSON schemas under `docs/spec/` (for example, `dns_record.schema.json` and `fee_v2.schema.json`) document public payloads; cross-language vectors live in tests and fuzz targets (`fuzz/rpc`, `explorer/tests`).
- Hash layout and binary struct helpers live in `node/src/util/binary_struct.rs` and `node/src/util/binary_codec.rs`. Production crates use the serialization facade; `serde_json` and `bincode` appear only in tooling.

### Proof of Work and Service
- The hybrid PoW/PoS engine lives under `node/src/consensus`. `pow.rs` covers hash-based leaders, `pos.rs` handles stake selection, and `leader.rs` coordinates their votes before block assembly.
- Service-aware weighting feeds through `node/src/service_badge.rs`; badge-earned weight modifies scheduler fairness plus governance quorum checks.
- `node/src/exec.rs` binds work proofs into block production, ensuring compute/storage receipts attach directly to the coinbase ledger entries.

### Sharding
- Per-shard state roots are tracked and finalized in macro blocks. Inter-shard coordination, including cross-shard dependencies and reorg handling, lives in `node/src/blockchain/inter_shard.rs` with tests in `node/src/blockchain/tests`.
- Shard identifiers and layout are defined alongside ledger codecs; helper types are under `ledger::address::ShardId`.

### Difficulty and Proof of History
- `node/src/consensus/difficulty*.rs` implement Kalman retargeting with clamped deltas. VDF checkpoints feed `node/src/poh.rs` so propagation remains deterministic even under adversarial timing.
- PoH ticks emit telemetry and are replayed by `tests/poh.rs` plus the Python harness under `demo.py`.

### Macro Blocks and Finality
- `node/src/consensus/finality.rs` collects validator attestations, rotates stakes, and records dispute evidence in sled (`state/`).
- The DKG helper crate `dkg/` plus `node/src/dkg.rs` coordinates committee key refresh without exposing transcripts.

## Transaction and Execution Pipeline
### Transaction Lifecycle
- `node/src/transaction.rs` and `node/src/tx` encode canonical transaction envelopes shared with CLI/explorer via `foundation_serialization`. Account abstraction hooks (`docs/account_abstraction.md` equivalent) now live in `node/src/identity/handle_registry.rs` and `node/src/transaction/fee.rs`.
- Pipeline: mempool admission → QoS lanes → scheduler → execution → receipts anchored in ledger.

### Fee Lanes and Rebates
- Fee lanes are typed via `node/src/transaction::FeeLane` and `node/src/fee`, with rebate hooks under `node/src/fees` and `node/src/fee/readiness`. Governance controls floors through `governance/src/params.rs` and telemetry tracks enforcement (`gateway_fee_floor_*` metrics).
- Rebates post ledger entries that auto-apply to the submitter before consuming liquid CT. Reference detail lives in `docs/economics_and_governance.md#fee-lanes-and-rebates`.

### Mempool Admission and Eviction
- Admission and QoS live under `node/src/mempool/admission.rs`; scoring and eviction policies are in `node/src/mempool/scoring.rs`. Tests live in `node/src/mempool/tests`.
- Fee floors and EIP‑1559‑style base fee nudges are applied per block; telemetry exposes `mempool_fee_floor_*` and target fullness gauges.

### Scheduler and Parallel Execution
- `node/src/scheduler.rs` coordinates lane-aware batches with fairness timeouts. Workloads feed into `node/src/parallel.rs` so CPU-heavy tasks (GPU hashing, SNARK verification) stay deterministic.
- The compute scheduler reuses the same fairness machinery via `node/src/compute_market/scheduler` and `workloads.rs`.

### Virtual Machine and WASM
- `node/src/vm` embeds the bytecode VM, while WASM execution and debugging helpers sit in `node/src/vm/debugger.rs` plus `docs/developer_handbook.md#contract-and-vm-development`.
- Contracts interact with both UTXO and account space; CLI helpers live in `cli/src/wasm.rs` and `cli/src/contract_dev.rs`.

### Account Abstraction and Identity
- Distributed handles, DIDs, and registry logic live in `node/src/identity`. Binary codecs for handles/DIDs ensure explorers, wallets, and RPC share the same storage bytes.
- Light clients rely on this identity layer for DID revocation proofs and remote signer flows (`node/src/light_client`).

## Networking and Propagation
### P2P Handshake
- `node/src/p2p/handshake.rs` negotiates capabilities, runtime/transport providers, and telemetry hooks. Peer identity lives in the `p2p_overlay` crate with in-house and stub adapters.
- Capability negotiation exposes compression, service roles, and QUIC certificate fingerprints so gossip and RPC choose the right transport.

### P2P Wire Protocol
- Message framing and compatibility shims live under `node/src/p2p/wire_binary.rs`. Versioned encodings ensure older/minor peers interoperate; tests assert round-trip and legacy compatibility.

### QUIC Transport
- The transport crate (`crates/transport`) exposes provider traits with backends for Quinn and s2n (feature-gated) plus an in-house stub for tests. Providers advertise capabilities to the handshake layer.
- TLS configuration is applied per provider during instance creation (e.g., `apply_quinn_tls`, `apply_s2n_tls`), with resets ensuring only one provider’s TLS stack is active at a time.
- Callbacks propagate connect/disconnect/handshake statistics into telemetry for dashboards and incident analysis.

### Overlay and Peer Persistence
- Overlay persistence relies on `SimpleDb` namespaces (`node/src/net/peer.rs`, `net/overlay_store`). Operators migrate peer DBs via `scripts/migrate_overlay_store.rs` with guidance captured in `docs/operations.md#overlay-stores`.
- Uptime accounting flows through `p2p_overlay::uptime`; governance reward issuances reuse the same sled-backed snapshots.

### Gossip Relay
- `node/src/gossip/relay.rs` implements TTL-bound dedup, shard-aware peer sets, and latency + reputation scoring. Fanout metrics live in `node/src/telemetry.rs` (`GOSSIP_*` series) and the relay persists shard membership so partitions recover quickly.
- Range-boost deliveries and ANN payloads register as gossip hops, keeping mesh telemetry side-by-side with QUIC counts.

### QUIC Transport
- The in-house transport crate (`crates/transport`) abstracts Quinn and s2n providers. `node/src/net/quic.rs` publishes diag snapshots through RPC/CLI (`tb-cli net quic-stats`).
- Mutual-TLS materials derive from node keys, are cached, and rotate via governance toggles. Chaos tooling lives in `docs/operations.md#chaos-and-fault-drills`.

### LocalNet and Range Boost
- Device-to-device mesh lives in `node/src/localnet` (proximity proofs) and `node/src/range_boost` (queue, forwarder, telemetry). CLI toggles match env vars `TB_MESH_STATIC_PEERS` & `--range-boost`.
- Range boost ties into ad-market ANN snapshots: `node/src/ad_policy_snapshot.rs` persists signed JSON + `.sig` files for operator audits.

### Network Recovery and Topologies
- Partition detection sits in `node/src/net/partition_watch.rs`; remediation helpers live in `docs/operations.md#network-recovery` and CLI commands under `cli/src/remediation.rs`.
- A* routing heuristics, swarm presets, and bootstrap flow are summarized from the former `docs/net_a_star.md`, `docs/swarm.md`, `docs/net_bootstrap.md`, and `docs/network_topologies.md` into this section.

## Storage and State
### Storage Pipeline
- `node/src/storage/pipeline.rs` handles chunk sizing, erasure coding, encryption/compression selection, and provider placement. `coding/` supplies the compressor/erasure backends with runtime switches recorded in telemetry.
- Manifest handling uses `manifest_binary.rs` and `pipeline/binary` for compatibility across CLI/SDK.

### Storage Market
- `storage_market/` unifies sled, RocksDB, and memory via the `storage_engine` crate and the new policy layer. Rent escrows, provider profiles, and governance overrides for redundancy all sit here.
- Proof-of-retrievability, chunk repair, and simulator hooks now share the same store (see `node/src/storage/repair.rs`).

### SimpleDb and Storage Engines
- `node/src/simple_db` wraps the `storage_engine` traits; engines include in-house, RocksDB (feature-gated), and a memory engine for lightweight integration. Runtime selection is governed by `EngineConfig` and per-name overrides.
- Snapshot rewrites atomically replace column families using fsync’d temp files.
- The sled store remains in use for dedicated subsystems (for example, governance and explorer stores via the `sled/` crate), but it is not a SimpleDb backend.
- See also `state/README.md` and `docs/operations.md#storage-snapshots-and-wal-management` for crash replay and compaction guidance.

### Snapshots and State Pruning
- WAL + snapshot lifecycle is inside `node/src/storage/wal.rs`, `docs/operations.md#wal-and-snapshots`, and CLI commands `tb-cli snapshots ...`.
- State pruning logic lives under `node/src/state_pruning.rs`; governance knobs guard pruning depth and compaction windows.

### Repair and Simulation
- `node/src/storage/repair` + `docs/operations.md#storage-repair` outline provider scoring, erasure thresholds, and CLI triggers.
- Simulation harnesses (`docs/simulation_framework.md` content) now live here with references to `sim/` and `fuzz/` suites.

### Schema Migrations
- On-disk schema changes are introduced behind version bumps and lossless migrations. Historical notes from `docs/schema_migrations/*` are consolidated here and inline in code where applicable.
- Examples: bridge header persistence (v8), DEX escrow (v9), and industrial subsidies (v10). Migrations run during startup with telemetry for progress and error handling.

## Compute Marketplace
### Offers and Matching
- Computation lives under `node/src/compute_market`. Offers, bids, and receipts serialize through `foundation_serialization` and are exposed over RPC (`node/src/rpc/compute_market.rs`).
- Providers stake bonds (`compute_market::Offer`), schedule workloads, and settle receipts via `compute_market::settlement`.

### Lane Scheduler
- The matcher rotates fairness windows per lane and is backed by sled state stored under `state/market`. Lane telemetrics feed `match_loop_latency_seconds{lane}`.
- SLA slashing is being layered atop the same scheduler per `AGENTS.md §15.B`: failed workloads will emit slash receipts anchored in CT subsidy sub-ledgers, remediation dashboards (Grafana panels sourced from `monitoring/`) will highlight degraded lanes, and deterministic replay tests will cover fairness windows, starvation protection, and persisted receipts.

### Workloads and SNARK Receipts
- Supported workloads: transcode, inference, GPU hash, SNARK. SNARK proofs now run through `node/src/compute_market/snark.rs`, which wraps the Groth16 backend, hashes wasm bytes into circuit digests, caches compiled shapes per digest, and chooses CPU/GPU provers (with telemetry exported via `snark_prover_latency_seconds{backend}` / `snark_prover_failure_total{backend}`).
- Proof bundles carry circuit/output/witness commitments and serialized proof bytes; they are attached to SLA records in `compute_market::settlement` and surfaced over RPC via `compute_market.sla_history`.
- Explorer ingest mirrors the same payloads: `tb-cli explorer sync-proofs --db explorer.db --url http://node:26658` streams `compute_market.sla_history(limit)` responses, persists the serialized `Vec<ProofBundle>` per job (`compute_sla_proofs` table), and exposes them under `/compute/sla/history` so dashboards can render fingerprints/artifacts without talking to the node.
- Providers that advertise CUDA/ROCm GPUs (or dedicated accelerators) automatically attempt GPU proving first; failures fall back to CPU while feeding scheduler accelerator telemetry so providers can be reweighted.
- Benchmark harnesses for the prover live under `node/src/compute_market/tests/prover.rs` so operators can compare CPU/GPU latency locally before enabling accelerators.

### Courier and Replay
- Retry/courier logic (`node/src/compute_market/courier.rs`) persists inflight bundles so restarts resume outstanding work only.
- `docs/compute_market_courier.md` content moved here; CLI commands under `cli/src/compute.rs` manage the queue.

### Compute-backed Money (CBM)
- CBM hooks live in `node/src/compute_market/cbm.rs`. Governance toggles lane payouts, refundable deposits, and SLA slashing (`compute_market::settlement::SlaOutcome`).

## Energy Market
- Energy credits live in `crates/energy-market` with the node wrapper in `node/src/energy.rs`. Providers, credits, and receipts persist in sled via `SimpleDb::open_named(names::ENERGY_MARKET, …)`; set `TB_ENERGY_MARKET_DIR` to relocate the DB. The store snapshots to bytes (`EnergyMarket::{to_bytes,from_bytes}`) on every mutation and uses the same fsync+rename discipline as other `SimpleDb` consumers so restarts replay identical state.
- Oracle trust roots are defined in `config/default.toml` under `energy.provider_keys`. Each entry maps a provider ID to a 32-byte Ed25519 public key; reloads hot-swap the verifier registry via `node::energy::configure_provider_keys` so operators can rotate or revoke keys without restarts.
- RPC wiring (`node/src/rpc/energy.rs`) exposes `energy.register_provider`, `energy.market_state`, `energy.submit_reading`, `energy.settle`, `energy.receipts`, `energy.credits`, `energy.disputes`, `energy.flag_dispute`, and `energy.resolve_dispute`. The CLI (`cli/src/energy.rs`) emits the same JSON schema and prints providers, receipts, credits, and disputes so oracle adapters (`crates/oracle-adapter`) and explorers stay aligned. `docs/testnet/ENERGY_QUICKSTART.md` covers bootstrap, signature validation, dispute rehearsal, and how to script `tb-cli energy` calls.
- Governance owns `energy_min_stake`, `energy_oracle_timeout_blocks`, and `energy_slashing_rate_bps`. Proposals feed those values through the shared governance crate, latch them in `node/src/governance/params.rs`, then invoke `node::energy::set_governance_params`, so runtime hooks refresh the market config plus treasury/slashing math with no recompiles.
- Observability: `energy_market` emits gauges (`energy_provider_total`, `energy_pending_credits_total`, `energy_receipt_total`, `energy_active_disputes_total`, `energy_avg_price`), counters (`energy_provider_register_total`, `energy_meter_reading_total{provider}`, `energy_settlement_total{provider}`, `energy_treasury_fee_ct_total`, `energy_dispute_{open,resolve}_total`, `energy_kwh_traded_total`, `energy_signature_failure_total{provider,reason}`), histograms (`energy_provider_fulfillment_ms`, `oracle_reading_latency_seconds`), and simple health probes (`node::energy::check_energy_market_health`). Feed them into the metrics-aggregator dashboards and alert whenever pending meter credits exceed the safe envelope or signature failures spike.

### Energy, Governance, and RPC Next Tasks
- **Governance + Params**
  - Add proposal payloads for energy bundles (batch vs real-time settle) with `ParamSpec` + runtime hooks.
  - Wire explorer + CLI timelines so energy param changes and activation/rollback history stay visible.
  - Expand dependency graph support in proposals (deps validation in the node mirror + conflict tests).
  - Harden param persistence snapshots and rollback audits with more regression coverage.
- **Energy + Oracle**
  - Ed25519 verification now lives inside `oracle-adapter` (`Ed25519SignatureVerifier`) with provider-key registration so adapters reject unsigned readings. Provider keys load from `energy.provider_keys` in the node config and propagate into the sled-backed verifier registry automatically. Remaining work focuses on oracle quorum/expiry policies, ledger anchoring, and advanced telemetry.
  - Add oracle quorum/expiry policy (multi-reading attestation) with richer slashing telemetry.
  - Persist energy receipts to ledger anchors or dedicated sled trees with replay tests.
  - Expand CLI/ explorer flows for provider updates (price, stake top-up) once governance exposes the payloads.
- **RPC + CLI Hardening**
  - Add RPC auth + rate limiting specific to the `energy.*` endpoints (aligned with gateway policy).
  - Cover negative cases + structured errors for `energy.submit_reading` (bad signature, stale timestamp, wrong meter) and the new dispute endpoints.
  - Publish JSON schema snippets for energy payloads/oracle messages plus round-trip CLI tests.
- **Telemetry + Observability**
  - Extend Grafana dashboards: provider count, pending credits, dispute trends, settlement rate, slash totals.
  - Add SLOs/alerts for oracle latency, slashing spikes, settlement stalls, and dispute backlog.
  - Wire metrics-aggregator summary endpoints so `/wrappers` and `/telemetry/summary` expose the new energy stats.
- **Network + Transport**
  - Run QUIC chaos drills with per-provider failover simulation + fingerprint rotation tests.
  - Add handshake capability assertions in `node/tests` for the new transport metadata paths.
- **Storage + State**
  - Mirror `SimpleDb` snapshots for energy (`TB_ENERGY_MARKET_DIR`) with fsync+atomic swap and document restore flow.
  - Ship migration drill scripts/tests for energy schema evolution (backwards compatibility).
- **Security + Supply Chain**
  - Enforce release provenance gates for energy/oracle crates (vendor snapshot + checksums in CI).
  - Tighten oracle adapter secret hygiene (key sourcing, redaction) + boundary fuzz tests for decoding.
- **Performance + Correctness**
  - Throughput benchmarks for meter ingestion + settlement (per-provider histograms).
  - Fuzzers for the energy binary codec, RPC param decoding, and governance activation queue.
  - Deterministic replay in CI for energy receipt reapplication across x86_64/AArch64.
- **Docs + Explorer**
  - Explorer views: provider table, receipts timeline, fee/slash summaries, plus SQLite schema updates.
  - Expand `docs/testnet/ENERGY_QUICKSTART.md` with dispute flows + verifier integration.
- **CI + Test Suite**
  - Stabilize the full integration suite and gate merges on: governance-param wiring, RPC energy, handshake, rate limiters, ad-market RPC.
  - Add a “fast mainnet gate” workflow that runs: unit tests + targeted integration (governance, RPC, ledger replay, transport handshake).

## Bridges, DEX, and Settlement
### Token Bridges
- The `bridges/` crate handles POW header verification, relayer sets, telemetry, and dispute handling. RPC wiring lives in `node/src/rpc/bridge.rs`.
- Verified headers persist in sled (schema migration v8) and CLI commands under `cli/src/bridge.rs` manage challenge windows.
- Release-verifier tooling now tracks relayer payload attestations, signer-set rotations, and escrow proof exports per `AGENTS.md §15.E`. Every bridge change must update `docs/security_and_privacy.md#release-provenance-and-supply-chain`, emit telemetry counters (`bridge_signer_rotation_total`, `bridge_partial_payment_retry_total`), and keep explorer dashboards aligned with the canonical snapshot JSON produced by the CLI.

### DEX and Trust Lines
- `node/src/dex` + `dex/` supply order books, trust-line routing, escrow constraints, and adapters (Uniswap/Osmosis). Trust-line state is sled-backed and streamed to explorers/CLI.
- Deterministic replay coverage for escrow settlement and AMM invariants plus telemetry for multi-hop routing latency, escrow fulfillment, and signer rotations are required by the same `AGENTS.md §15.E` directive so dashboards and operators never diverge from node state.

### HTLC and Cross-Chain
- Atomic swap primitives (`docs/htlc_swaps.md` replacement) were folded into `node/src/dex/htlc.rs` with RPC + CLI helpers. Governance tracks lane quotas and telemetry under `DEX_*` metrics.

## Gateway and Client Access
### HTTP Gateway
- `node/src/gateway/http.rs` uses `crates/httpd` for the router, TLS, and WebSocket upgrades. Gateways serve static content, APIs, and compute relays from the embedded storage pipeline.
- CLI + explorer insight commands surfaced from old `docs/gateway.md` now live in `docs/apis_and_tooling.md#gateway`.

### DNS Publishing
- DNS + `.block` records are handled by `node/src/gateway/dns.rs` with schemas archived under `docs/spec/dns_record.schema.json`.

### DNS Auctions and Staking
- Gateway domain auctions use stake-backed bids and escrowed CT recorded under `node/src/gateway/dns.rs` (see `StakeEscrowRecord`). RPC/CLI support deposit, withdraw, and refund flows with error codes under the same module.

### Mobile Gateway Cache
- Mobile caches persist ChaCha20-Poly1305 encrypted blobs in sled (`node/src/gateway/mobile_cache.rs`). TTL sweeps and CLI flush commands ensure offline support without stale data.

### Light Clients
- `node/src/light_client` streams headers, DID updates, and proofs. Streaming endpoints live in `node/src/rpc/state_stream.rs` and CLI commands under `cli/src/light_sync.rs`.
- Mobile updates plus power/bandwidth heuristics from the old `docs/mobile_light_client.md` live here and in `docs/apis_and_tooling.md#light-client-streaming`.

### Read Receipts
- `node/src/gateway/read_receipt.rs` records signed acknowledgements, batches them for ledger inclusion, and exposes CLI/metrics counters. Economics for `READ_SUB_CT` live in `docs/economics_and_governance.md`.

## Telemetry and Instrumentation
### Runtime Telemetry
- `node/src/telemetry.rs` registers every metric (TLS warnings, coding results, gossip fanout, SLA counters). CLI + aggregator share the same registry via `runtime::telemetry`.
- Wrapper telemetry exports runtime/transport/overlay/storage/coding metadata so governance policy violations are visible.

### Metrics Aggregator
- `metrics-aggregator/` collects node metrics, correlates them, exposes TLS warning audits, bridge remediation, and governance telemetry. HTTP endpoints live in the same `httpd` router, and optional S3 uploads reuse `foundation_object_store`.

### Monitoring Stack
- `monitoring/` provides Grafana dashboards and Prometheus rules. JSON dashboards (e.g., `monitoring/compute_market_dashboard.json`) are kept in-tree; see `docs/operations.md#monitoring` for install steps.

## Auxiliary Services
### Service Badges
- `node/src/service_badge.rs` tracks uptime, latency, renewals, and issuance/revocation logic. Governance toggles TTL, uptime thresholds, and telemetry is emitted as `BADGE_*` counters.

### Ad Marketplace
- Ad market crates (`ad_market`, `node/src/ad_policy_snapshot.rs`, `node/src/ad_readiness.rs`) manage policy snapshots, ANN proofs, conversion tokens, and mesh deliveries. CLI + RPC surfaces sit in `cli/src/ad_market.rs` and `node/src/rpc/ad_market.rs`.

### Law-enforcement Portal and Jurisdiction Packs
- LE logging (`node/src/le_portal.rs`) records requests, actions, canaries, and evidence logs, with privacy redaction optional. Jurisdiction packs (`jurisdiction/`, `docs/security_and_privacy.md#jurisdiction-packs`) scope consent defaults and audit hooks.

### Range-Boost and LocalNet Telemetry
- Mesh queue depth, hop latency, and fault toggles are exported via `node/src/range_boost` metrics. Operators manage peers and mesh policies through the CLI + `docs/operations.md#range-boost`.
