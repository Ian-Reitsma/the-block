# Architecture

Everything below reflects what ships in `main` today. Paths reference the exact modules so engineers can cross-check behaviour while hacking.

> **For newcomers:** This doc is technical. Each major section starts with a plain-language explainer. If you want a gentler intro, read [`docs/overview.md`](overview.md) first.

## Ledger and Consensus

> **Plain English:** The ledger is the shared spreadsheet everyone agrees on. It tracks who owns what BLOCK and what services are owed. Blocks are like pages — every ~1 second, a new page is added containing recent transactions. "Consensus" is how all the computers (nodes) agree on which page comes next, preventing anyone from cheating.
>
> **Key concepts:**
> - **Regular blocks**: Added every ~1 second
> - **Macro-blocks**: Periodic checkpoints (every N blocks) that summarize state and make syncing faster
> - **State roots**: Cryptographic fingerprints that prove what's in the ledger without showing everything

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

> **Plain English:** When you want to send BLOCK or use a service, you create a "transaction" — a signed message saying what you want to do. Here's the journey:
>
> 1. **You sign it** — Your wallet creates and signs the transaction
> 2. **It enters the mempool** — The "waiting room" where transactions sit before being included in a block
> 3. **The scheduler picks it** — Transactions are batched by priority (higher fees = faster)
> 4. **It gets executed** — The node runs the transaction, updating balances
> 5. **A receipt is created** — Proof that it happened, anchored in the ledger
>
> **Fee lanes** are like different queues at the post office: regular, priority, special services. Each has rules and pricing.

### Transaction Lifecycle
- `node/src/transaction.rs` and `node/src/tx` encode canonical transaction envelopes shared with CLI/explorer via `foundation_serialization`. Account abstraction hooks (`docs/account_abstraction.md` equivalent) now live in `node/src/identity/handle_registry.rs` and `node/src/transaction/fee.rs`.
- Pipeline: mempool admission → QoS lanes → scheduler → execution → receipts anchored in ledger.

### Fee Lanes and Rebates
- Fee lanes are typed via `node/src/transaction::FeeLane` and `node/src/fee`, with rebate hooks under `node/src/fees` and `node/src/fee/readiness`. Governance controls floors through `governance/src/params.rs` and telemetry tracks enforcement (`fee_floor_warning_total`, `fee_floor_override_total`, `fee_floor_reject_total`).
- Rebates post ledger entries that auto-apply to the submitter before consuming liquid BLOCK. Reference detail lives in `docs/economics_and_governance.md#fee-lanes-and-rebates`.

### Mempool Admission and Eviction
- Admission and QoS live under `node/src/mempool/admission.rs`; scoring and eviction policies are in `node/src/mempool/scoring.rs`. Tests live in `node/src/mempool/tests`.
- Fee floors and EIP‑1559‑style base fee nudges are applied per block; telemetry exposes `fee_floor_current` plus per‑lane warning/override counters, and `mempool.stats` surfaces per‑lane floors for RPC/CLI consumers.

### Scheduler and Parallel Execution
- `node/src/scheduler.rs` coordinates lane-aware batches with fairness timeouts. Workloads feed into `node/src/parallel.rs` so CPU-heavy tasks (GPU hashing, SNARK verification) stay deterministic.
- The compute scheduler reuses the same fairness machinery via `node/src/compute_market/scheduler` and `workloads.rs`.

### Virtual Machine and WASM
- `node/src/vm` embeds the bytecode VM, while WASM execution and debugging helpers sit in `node/src/vm/debugger.rs` plus `docs/developer_handbook.md#contract-and-vm-development`.
- Contracts interact with both UTXO and account space; CLI helpers live in `cli/src/wasm.rs` and `cli/src/contract_dev.rs`.

### Account Abstraction and Identity
- Distributed handles, DIDs, and registry logic live in `node/src/identity`. Binary codecs for handles/DIDs ensure explorers, wallets, and RPC share the same storage bytes.
- Light clients rely on this identity layer for DID revocation proofs and remote signer flows (`node/src/light_client`).

## Market Receipts and Audit Trail

> **Plain English:** Every time a market settles (storage provided, compute executed, energy delivered, ad shown), the system creates a "receipt" — permanent proof of what happened. These receipts:
> 1. **Live on-chain** — Stored in every block
> 2. **Prove consensus** — Included in block hash so nodes validate them
> 3. **Drive economics** — Launch Governor uses receipts to measure real market activity
> 4. **Enable auditing** — Anyone can replay the chain and verify all settlements
>
> **Why this matters:** Without receipts in consensus, malicious nodes could lie about market activity. With receipts, the entire network validates every settlement.

### Receipt Types and Schema

**Four market receipt types exist** (`node/src/receipts.rs`):

#### Storage Receipt
```rust
pub struct StorageReceipt {
    pub file_id: String,           // Unique file identifier
    pub provider: String,          // Storage provider address
    pub bytes_stored: u64,         // Total bytes in this settlement
    pub cost: u64,              // BLOCK paid to provider
    pub block_height: u64,         // When this settled
    pub duration_epochs: u32,      // How many epochs of storage
}
```
**Tracks:** File storage settlements, bytes delivered, provider compensation

#### Compute Receipt
```rust
pub struct ComputeReceipt {
    pub job_id: String,            // Unique job identifier
    pub worker: String,            // Compute provider address
    pub compute_units: u64,        // Units consumed
    pub cost: u64,              // BLOCK paid to worker
    pub block_height: u64,         // When job completed
    pub proof_type: String,        // "snark", "trusted", etc.
}
```
**Tracks:** Computation jobs, resource usage, verification method

#### Energy Receipt
```rust
pub struct EnergyReceipt {
    pub meter_id: String,          // Smart meter identifier
    pub provider: String,          // Energy provider address
    pub kwh_delivered: u64,        // Energy delivered (in milliwatt-hours)
    pub cost: u64,              // BLOCK paid to provider
    pub block_height: u64,         // Settlement block
    pub oracle_signature: Vec<u8>, // Oracle attestation
}
```
**Tracks:** Energy delivery, meter readings, oracle verification

#### Ad Receipt
```rust
pub struct AdReceipt {
    pub campaign_id: String,       // Campaign identifier
    pub creative_id: String,       // Creative identifier (per-campaign)
    pub publisher: String,         // Publisher address
    pub impressions: u64,          // Impressions delivered
    pub spend: u64,                // BLOCK spent by advertiser
    pub block_height: u64,         // Settlement block
    pub conversions: u32,          // Attributed conversions
    pub claim_routes: HashMap<String, String>, // Optional payout overrides per role
    pub role_breakdown: Option<AdRoleBreakdown>, // Optional role splits (viewer/host/etc.)
    pub device_links: Vec<DeviceLinkOptIn>, // Optional opt-in device-link attestations
    pub publisher_signature: Vec<u8>, // Publisher signature over receipt fields
    pub signature_nonce: u64,      // Nonce to prevent replay
}
```
```rust
pub struct AdRoleBreakdown {
    pub viewer: u64,
    pub host: u64,
    pub hardware: u64,
    pub verifier: u64,
    pub liquidity: u64,
    pub miner: u64,
    pub price_usd_micros: u64,
    pub clearing_price_usd_micros: u64,
}

pub struct DeviceLinkOptIn {
    pub device_hash: String,
    pub opt_in: bool,
}
```
**Tracks:** Ad delivery, impressions, spend, attribution, claim routing, and optional device-link attestations

Conversion events recorded via `ad_market.record_conversion` accumulate per `(campaign_id, creative_id)`; block assembly drains the counts into `AdReceipt.conversions` (with optional `device_links` for opt-in dedup/attribution) so replay/economics stay deterministic even if the market restarts. Device-link dedup is best-effort within the persisted marketplace window and ignores non-opt-in payloads.

### Receipt Lifecycle

**End-to-End Flow:**

```
┌─────────────────┐
│ Market Activity │  (Storage/Compute/Energy/Ad)
│  (off-chain)    │
└────────┬────────┘
         │
         v
┌─────────────────┐
│ Market Contract │  Validates settlement
│  Settlement     │  Creates Receipt struct
└────────┬────────┘
         │
         v
┌─────────────────┐
│ Pending Buffer  │  Market holds receipts during epoch
│ (per market)    │  Accumulates all settlements
└────────┬────────┘
         │
         v
┌─────────────────┐
│ Block Assembly  │  Miner collects all pending receipts
│  (consensus)    │  Serializes via encode_receipts()
└────────┬────────┘
         │
         v
┌─────────────────┐
│ Block Hash      │  receipts_serialized included in BLAKE3
│  Calculation    │  Makes receipts consensus-critical
└────────┬────────┘
         │
         v
┌─────────────────┐
│ Block Broadcast │  Full block with receipts propagates
│  (gossip)       │  All nodes validate receipts via hash
└────────┬────────┘
         │
         v
┌─────────────────┐
│ Telemetry       │  receipt_storage_total++
│  Recording      │  receipt_bytes_per_block updated
└────────┬────────┘
         │
         v
┌─────────────────┐
│ Metrics Engine  │  Derives market utilization
│  Derivation     │  Calculates provider margins
└────────┬────────┘
         │
         v
┌─────────────────┐
│ Launch Governor │  Uses metrics for economic gates
│  Consumption    │  Adjusts subsidy allocation
└─────────────────┘
```

### Receipt Header, Sharding, and Availability

- Receipt commitments are sharded across `receipt_shard_count` buckets; every block includes a `receipt_header` with per-shard roots, blob commitments, DA expiry (`available_until`), aggregate signature digest, and the shard count. Activation height defaults to `0` (always on); knobs live in `NodeConfig` (`receipt_shard_count`, `receipt_blob_da_window_secs`, `receipt_max_per_provider_per_shard`, `receipt_min_region_diversity`, `receipt_min_asn_diversity`, `receipt_header_activation_height`).
- Shard-level and total budgets are enforced via `ReceiptShardAccumulator`; diversity checks cap receipts per provider/publisher per shard and require distinct regions/ASNs before proposal/validation succeeds. Validation recomputes roots/digest from delivered receipts and rejects expired headers.
- Receipts missing region/ASN metadata are counted under an `unknown` placeholder so the baseline `min_region_diversity`/`min_asn_diversity` requirement of `1` does not reject empty registries; thresholds above `1` still require distinct populated values.
- Aggregate signature is currently a batch-Ed25519 digest over high-volume receipts (ad/energy). Swap in a true aggregation backend under the existing `aggregate_scheme` enum when available.
- Blob commitments are zero placeholders until the blob chain DA pointers land; the vector ordering matches shard roots so proofs can drop in without schema changes.

### Consensus Integration

**Block Hash Calculation** (`node/src/hashlayout.rs`):

```rust
pub struct BlockEncoder<'a> {
    // ... existing fields ...
    pub receipts_serialized: &'a [u8],  // Added December 2025
}

impl<'a> HashEncoder for BlockEncoder<'a> {
    fn encode(&self, h: &mut Hasher) {
        // ... hash all block fields ...
        
        // Consensus-critical: receipts affect block hash
        h.update(&(self.receipts_serialized.len() as u32).to_le_bytes());
        h.update(self.receipts_serialized);
        
        // ... continue hashing ...
    }
}
```

**Why receipts are in the hash:**
- Prevents nodes from lying about market activity
- Enables deterministic metrics derivation
- Makes receipt tampering result in hash mismatch (rejected block)
- Allows lightweight receipt verification (just check block hash)

**Block Construction Pattern:**
```rust
// 1. Collect receipts from all markets
let mut receipts = Vec::new();
receipts.extend(storage_market.drain_pending_receipts());
receipts.extend(compute_market.drain_pending_receipts());
receipts.extend(energy_market.drain_pending_receipts());
receipts.extend(ad_market.drain_pending_receipts());

// 2. Serialize for consensus
let receipts_bytes = block_binary::encode_receipts(&receipts)?;

// 3. Build block encoder
let encoder = hashlayout::BlockEncoder {
    // ... other fields ...
    receipts_serialized: &receipts_bytes,
};

// 4. Calculate hash (includes receipts)
let hash = encoder.encode(&mut hasher);

// 5. Construct final block
Block {
    hash,
    receipts,  // Stored in block
    // ... other fields ...
}

#### Block Validation and Transaction Verification

- `node/src/blockchain/process.rs::validate_and_apply` no longer clones `Blockchain::accounts` wholesale. It copies each account lazily the first time a transaction touches it, records the touched addresses, and emits `StateDelta` entries for only the mutated accounts so block validation keeps working set size proportional to per-block activity rather than the entire universe.
- `node/src/transaction.rs::verify_signed_tx` reuses the canonical payload bytes when building the domain-separated message, and the cache key now hashes that message plus the signing/public-key material in one pass. Deduplicated hashing removes the redundant BLAKE3 pass that previously serialized and hashed the entire transaction while still mapping each unique signed transaction to a stable `[u8; 32]` cache key.
```

### Receipt Serialization

**Binary Format** (`node/src/block_binary.rs`):

```rust
pub fn encode_receipts(receipts: &[Receipt]) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(receipts.len() * 256);
    write_receipts(&mut writer, receipts)?;
    Ok(writer.finish())
}

fn write_receipts(writer: &mut Writer, receipts: &[Receipt]) -> EncodeResult<()> {
    writer.write_array(receipts.len(), |array_writer| {
        for receipt in receipts {
            array_writer.item_with(|item_writer| {
                write_receipt(item_writer, receipt)
            });
        }
    });
    Ok(())
}
```

**Determinism guarantees:**
- Fixed field order (no HashMap iteration)
- Length-prefixed arrays
- Platform-independent encoding (no native integers)
- Same serialization across all nodes

### Telemetry and Observability

**Receipt Metrics** (`node/src/telemetry/receipts.rs`):

| Metric | Type | Description |
|--------|------|-------------|
| `receipts_storage_total` | Counter | Total storage receipts processed |
| `receipts_compute_total` | Counter | Total compute receipts processed |
| `receipts_energy_total` | Counter | Total energy receipts processed |
| `receipts_ad_total` | Counter | Total ad receipts processed |
| `receipts_per_block` | Gauge | Receipts in current block |
| `receipts_storage_per_block` | Gauge | Storage receipts in current block |
| `receipts_compute_per_block` | Gauge | Compute receipts in current block |
| `receipts_energy_per_block` | Gauge | Energy receipts in current block |
| `receipts_ad_per_block` | Gauge | Ad receipts in current block |
| `receipt_bytes_per_block` | Gauge | Serialized receipt size (bytes) |
| `receipt_settlement_storage` | Gauge | Storage settlement amount (BLOCK) |
| `receipt_settlement_compute` | Gauge | Compute settlement amount (BLOCK) |
| `receipt_settlement_energy` | Gauge | Energy settlement amount (BLOCK) |
| `receipt_settlement_ad` | Gauge | Ad settlement amount (BLOCK) |
| `metrics_derivation_duration_ms` | Histogram | Time to derive metrics from receipts |
| `receipt_shard_count_per_block{shard}` | Gauge | Receipt count per shard |
| `receipt_shard_bytes_per_block{shard}` | Gauge | Serialized bytes per shard |
| `receipt_shard_verify_units_per_block{shard}` | Gauge | Verify units per shard |
| `receipt_da_sample_success_total` / `receipt_da_sample_failure_total` | Counter | DA sampling outcomes |
| `receipt_aggregate_sig_mismatch_total` | Counter | Aggregated signature/header mismatches |
| `receipt_header_mismatch_total` | Counter | Per-shard root/header mismatches |
| `receipt_shard_diversity_violation_total` | Counter | Provider/region/ASN diversity violations |

**Usage:**
```rust
#[cfg(feature = "telemetry")]
{
    let serialized = block_binary::encode_receipts(&block.receipts).unwrap_or_default();
    telemetry::receipts::record_receipts(&block.receipts, serialized.len());
}
```

### Economic Metrics Derivation

**Deterministic Engine** (`node/src/economics/deterministic_metrics.rs`):

```rust
pub fn derive_market_metrics_from_chain(
    blocks: &[Block],
) -> MarketMetrics {
    let mut metrics = MarketMetrics::default();
    
    for block in blocks {
        for receipt in &block.receipts {
            match receipt {
                Receipt::Storage(r) => {
                    metrics.storage_volume += r.bytes_stored;
                    metrics.storage_revenue += r.cost;
                },
                Receipt::Compute(r) => {
                    metrics.compute_units += r.compute_units;
                    metrics.compute_revenue += r.cost;
                },
                Receipt::Energy(r) => {
                    metrics.energy_kwh += r.kwh_delivered;
                    metrics.energy_revenue += r.cost;
                },
                Receipt::Ad(r) => {
                    metrics.ad_impressions += r.impressions;
                    metrics.ad_revenue += r.spend;
                },
            }
        }
    }
    
    // Calculate derived metrics
    metrics.storage_utilization = calculate_utilization(
        metrics.storage_volume,
        STORAGE_CAPACITY,
    );
    // ... other calculations ...
    
    metrics
}
```

**Launch Governor Integration:**

The Launch Governor consumes receipt-derived metrics:

```rust
// In launch_governor gate evaluation
let metrics = derive_market_metrics_from_chain(&recent_blocks);

if metrics.storage_utilization >= STORAGE_GATE_THRESHOLD
    && metrics.compute_utilization >= COMPUTE_GATE_THRESHOLD
    && metrics.energy_utilization >= ENERGY_GATE_THRESHOLD
    && metrics.ad_utilization >= AD_GATE_THRESHOLD
{
    // Activate economics gate
    create_intent(GateType::Economics, metrics);
}
```

See `docs/operations.md#receipt-telemetry` for Grafana dashboard setup and alerting.

### Implementation Status

**✅ Complete (December 2025):**
- Receipt type definitions (`node/src/receipts.rs`)
- Block serialization with receipts
- Consensus hash integration (`node/src/hashlayout.rs`)
- Telemetry system (`node/src/telemetry/receipts.rs`)
- Metrics derivation engine
- Integration tests
- Documentation

**⏳ In Progress:**
- BlockEncoder call site updates (manual grep + edit)
- Market receipt emission (ad, storage, compute, energy)
- Deployment to testnet

**See:** `RECEIPT_INTEGRATION_INDEX.md` for complete status, guides, and next steps.

## Networking and Propagation

> **Plain English:** Nodes need to talk to each other to share blocks and transactions. This section covers how they find each other, establish secure connections, and gossip information across the network.
>
> **Key concepts:**
> - **P2P (Peer-to-Peer)**: Nodes connect directly to each other, no central server
> - **QUIC**: A fast, modern transport protocol (like TCP but better for unreliable networks)
> - **Gossip**: How information spreads — each node tells a few others, who tell a few others, etc.
> - **Handshake**: The initial "hello" where nodes agree on capabilities and verify identities

### P2P Handshake
- `node/src/p2p/handshake.rs` negotiates capabilities, runtime/transport providers, and telemetry hooks. Peer identity lives in the `p2p_overlay` crate with in-house and stub adapters.
- Capability negotiation exposes compression, service roles, and QUIC certificate fingerprints so gossip and RPC choose the right transport.
- Handshake hellos now carry `gossip_addr` (the sender's gossip listener address); peers reply and push their chain snapshot to that address so restarts/joiners converge immediately without waiting for new blocks.
- Adding a peer triggers an immediate handshake + hello exchange so rejoined peers resync and refresh their peer lists without waiting on a new block.
- Inbound gossip is accepted on a non-blocking listener and processed on the blocking worker pool so chain validation cannot stall new connections. Chain sync uses explicit `ChainRequest` pulls (requesting from the local height) plus immediate snapshot pushes with exponential-backoff retries, and a periodic pull tick (default 500ms, `TB_P2P_CHAIN_SYNC_INTERVAL_MS`) to recover from missed broadcasts.
- QUIC certificates are required for QUIC transport; if a TCP-only peer advertises an invalid QUIC cert, the handshake proceeds but QUIC metadata is ignored and the peer stays on TCP.
- Certificate fingerprints are enforced for QUIC traffic (and for any message that includes a fingerprint); TCP fallbacks accept missing fingerprints even when a cached QUIC cert exists so mixed-transport peers can still converge.

### P2P Wire Protocol
- Message framing and compatibility shims live under `node/src/p2p/wire_binary.rs`. Versioned encodings ensure older/minor peers interoperate; tests assert round-trip and legacy compatibility.

### P2P Chain Synchronization
- **Messages:** `ChainRequest { from_height }` (pull) and `Chain(Vec<Block>)` (push) live in `node/src/net/peer.rs`/`node/src/net/mod.rs`. Requests ask peers to stream the suffix beyond `from_height`; responses bundle a coalesced segment.
- **Periodic pulls:** A dedicated tick thread triggers chain sync every `TB_P2P_CHAIN_SYNC_INTERVAL_MS` (default 500 ms) so nodes recover from missed broadcasts without waiting for new blocks.
- **Coalesced broadcast:** `schedule_chain_broadcast()` batches outgoing `Chain` messages and tracks `last_broadcast_len`/`last_broadcast_ms` to avoid floods while keeping lagging peers caught up.
- **Backoff + dedup:** Chain pushes retry with exponential backoff (`send_msg_with_backoff`) and track watermarks so duplicate payloads are dropped early.
- **PeerSet ownership:** The `PeerSet` now owns the signing key used for chain sync messages and validates peer key ownership during scheduling to prevent spoofed chain pushes.

### QUIC Transport
- The transport crate (`crates/transport`) exposes provider traits with backends for Quinn and s2n (feature-gated) plus an in-house stub for tests. Providers advertise capabilities to the handshake layer.
- TLS configuration is applied per provider during instance creation (e.g., `apply_quinn_tls`, `apply_s2n_tls`), with resets ensuring only one provider’s TLS stack is active at a time.
- Callbacks propagate connect/disconnect/handshake statistics into telemetry for dashboards and incident analysis.
- TLS handshake timeouts are enforced on both ends: `ServerConfig::tls_handshake_timeout` and `ClientConfig::tls_handshake_timeout` guard slowloris-style stalls and are exposed via `TB_TLS_HANDSHAKE_TIMEOUT_MS`.

### Runtime Reactor Configuration
- **Idle polling:** `REACTOR_IDLE_POLL_MS` (see `node/src/net/inhouse/mod.rs`) caps the sleep between polls to keep latency predictable. For throughput-sensitive deployments, lower values reduce tail latency at the cost of CPU.
- **Read/write backoff:** `io_read_backoff_ms` and `io_write_backoff_ms` introduce short sleeps when readiness hints are missed; write interest is tracked explicitly to avoid busy loops on bursty peers.
- **BSD kqueue hardening:** `node/src/net/platform_bsd.rs` now uses level-triggered mode (removing `EV_CLEAR`) and refreshes `update_interest()` when state changes to avoid missed wakeups under load.
- **Tuning guidance:** Raise backoff delays if CPU is saturated by idle peers; lower them (and idle poll) for low-latency testnets. Keep telemetry on (`runtime_read_without_ready_total`, `runtime_write_without_ready_total`) to validate the chosen settings.
- **Config reload fallback:** `node/src/config.rs` prefers inotify/kqueue watchers, but falls back to mtime polling if filesystem events fail. Expect up to one poll interval of delay when the fallback is active.

### Overlay and Peer Persistence
- Overlay persistence relies on `SimpleDb` namespaces (`node/src/net/peer.rs`, `net/overlay_store`). Operators migrate peer DBs via `scripts/migrate_overlay_store.rs` with guidance captured in `docs/operations.md#overlay-stores`.
- Uptime accounting flows through `p2p_overlay::uptime`; governance reward issuances reuse the same sled-backed snapshots.

### Gossip Relay
- `node/src/gossip/relay.rs` implements TTL-bound dedup, shard-aware peer sets, and latency + reputation scoring. Fanout metrics live in `node/src/telemetry.rs` (`GOSSIP_*` series) and the relay persists shard membership so partitions recover quickly.
- Range-boost deliveries and ANN payloads register as gossip hops, keeping mesh telemetry side-by-side with QUIC counts.
- P2P rate limiting clamps request volume with a configurable window: `TB_P2P_RATE_WINDOW_SECS` (default 1s) controls how long counts accumulate before reset, pairing with `TB_P2P_MAX_PER_SEC`/`TB_P2P_MAX_BYTES_PER_SEC` to keep abusive peers from starving the network. Operators can widen the window for incident drills or lock it down during attacks.

### QUIC Transport
- The in-house transport crate (`crates/transport`) abstracts Quinn and s2n providers. `node/src/net/quic.rs` publishes diag snapshots through RPC/CLI (`contract-cli net quic-stats`).
- Mutual-TLS materials derive from node keys, are cached, and rotate via governance toggles. Chaos tooling lives in `docs/operations.md#chaos-and-fault-drills`.

### LocalNet and Range Boost
- Device-to-device mesh lives in `node/src/localnet` (proximity proofs) and `node/src/range_boost` (queue, forwarder, telemetry). CLI toggles match env vars `TB_MESH_STATIC_PEERS` & `--range-boost`.
- Range boost ties into ad-market ANN snapshots: `node/src/ad_policy_snapshot.rs` persists signed JSON + `.sig` files for operator audits.

### Network Recovery and Topologies
- Partition detection sits in `node/src/net/partition_watch.rs`; remediation helpers live in `docs/operations.md#network-recovery` and CLI commands under `cli/src/remediation.rs`.
- A* routing heuristics, swarm presets, and bootstrap flow are summarized from the former `docs/net_a_star.md`, `docs/swarm.md`, `docs/net_bootstrap.md`, and `docs/network_topologies.md` into this section.

## Storage and State

> **Plain English:** The Block lets you store files in a decentralized way — like Dropbox, but no single company controls it. Files are:
> 1. **Chunked** — Split into pieces
> 2. **Encrypted** — So only you can read them
> 3. **Erasure coded** — Spread across multiple providers so the file survives even if some go offline
> 4. **Tracked on-chain** — The ledger knows who stores what and pays them BLOCK
>
> **SimpleDb** is our internal key-value store that handles crash-safe writes using atomic file operations.

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
- WAL + snapshot lifecycle is inside `node/src/storage/wal.rs`, `docs/operations.md#wal-and-snapshots`, and CLI commands `contract-cli snapshots ...`.
- State pruning logic lives under `node/src/state_pruning.rs`; governance knobs guard pruning depth and compaction windows.

### Repair and Simulation
- `node/src/storage/repair` + `docs/operations.md#storage-repair` outline provider scoring, erasure thresholds, and CLI triggers.
- Simulation harnesses (`docs/simulation_framework.md` content) now live here with references to `sim/` and `fuzz/` suites.

### Schema Migrations
- On-disk schema changes are introduced behind version bumps and lossless migrations. Historical notes are consolidated here, in `docs/system_reference.md#1-5-schema-migrations`, and inline in code where applicable.
- Examples: bridge header persistence (v8), DEX escrow (v9), and industrial subsidies (v10). Migrations run during startup with telemetry for progress and error handling.

## Compute Marketplace

> **Plain English:** Think of this as a built-in AWS marketplace where people sell compute time, and the blockchain can audit that the work actually got done.
>
> **How it works:**
> 1. **Provider offers compute** — "I have a GPU, I'll run your jobs for X BLOCK per hour"
> 2. **Consumer submits a job** — "Run this ML model on my data"
> 3. **Work gets done** — Provider executes the job
> 4. **SNARK receipt proves it** — A small cryptographic proof shows the work was done correctly, without re-running it
> 5. **BLOCK changes hands** — Provider gets paid, consumer gets results
>
> **Key terms:**
> - **Offer**: A provider's listing (price, capacity, bond deposited)
> - **SNARK receipt**: Proof that computation happened correctly
> - **SLA (Service Level Agreement)**: Rules about quality/uptime; violations can lead to slashing
> - **Lane**: Priority tier for different job types

**BlockTorch integration**: The `metal-backend/` stack (metal-tensor + autograd) provides the deterministic tensor layer for ML workloads executed through the compute marketplace. BlockTorch defines the kernel set, gradient serialization, and proof-ready metadata needed for SNARK attestation and pricing via `ORCHARD_TENSOR_PROFILE`. The strategic roadmap and coordinator workflow live in [`docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md`](ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md#part-xii-blocktorch--the-compute-framework-strategy), with execution priority captured in `AGENTS.md §15.B.1`.

### Offers and Matching
- Computation lives under `node/src/compute_market`. Offers, bids, and receipts serialize through `foundation_serialization` and are exposed over RPC (`node/src/rpc/compute_market.rs`).
- Providers stake bonds (`compute_market::Offer`), schedule workloads, and settle receipts via `compute_market::settlement`.

### Lane Scheduler
- The matcher rotates fairness windows per lane and is backed by sled state stored under `state/market`. Lane telemetrics feed `match_loop_latency_seconds{lane}`.
- SLA slashing is being layered atop the same scheduler per `AGENTS.md §15.B`: failed workloads will emit slash receipts anchored in BLOCK subsidy sub-ledgers, remediation dashboards (Grafana panels sourced from `monitoring/`) will highlight degraded lanes, and deterministic replay tests will cover fairness windows, starvation protection, and persisted receipts.

### Workloads and SNARK Receipts
- Supported workloads: transcode, inference, GPU hash, SNARK. SNARK proofs now run through `node/src/compute_market/snark.rs`, which wraps the Groth16 backend, hashes wasm bytes into circuit digests, caches compiled shapes per digest, and chooses CPU/GPU provers (with telemetry exported via `snark_prover_latency_seconds{backend}` / `snark_prover_failure_total{backend}`).
- Proof bundles carry circuit/output/witness commitments and serialized proof bytes; they are attached to SLA records in `compute_market::settlement` and surfaced over RPC via `compute_market.sla_history`.
- Explorer ingest mirrors the same payloads: `contract-cli explorer sync-proofs --db explorer.db --url http://node:26658` streams `compute_market.sla_history(limit)` responses, persists the serialized `Vec<ProofBundle>` per job (`compute_sla_proofs` table), and exposes them under `/compute/sla/history` so dashboards can render fingerprints/artifacts without talking to the node.
- Providers that advertise CUDA/ROCm GPUs (or dedicated accelerators) automatically attempt GPU proving first; failures fall back to CPU while feeding scheduler accelerator telemetry so providers can be reweighted.
- Benchmark harnesses for the prover live under `node/src/compute_market/tests/prover.rs` so operators can compare CPU/GPU latency locally before enabling accelerators.

### Courier and Replay
- Retry/courier logic (`node/src/compute_market/courier.rs`) persists inflight bundles so restarts resume outstanding work only.
- `docs/compute_market_courier.md` content moved here; CLI commands under `cli/src/compute.rs` manage the queue.

### Compute-backed Money (CBM)
- CBM hooks live in `node/src/compute_market/cbm.rs`. Governance toggles lane payouts, refundable deposits, and SLA slashing (`compute_market::settlement::SlaOutcome`).

## Energy Market

> **Plain English:** The energy market lets you buy and sell real-world electricity with built-in verification. Smart meters send cryptographically signed readings to the network, which turns them into "credits" that can be settled for BLOCK.
>
> **Example flow:**
> | Step | What Happens |
> |------|--------------|
> | 1. Register | Provider signs up with capacity (e.g., 10,000 kWh) and price (e.g., 50 BLOCK/kWh) |
> | 2. Meter reading | Smart meter sends signed reading: "1,000 kWh delivered" |
> | 3. Credit created | Network verifies signature, creates an `EnergyCredit` |
> | 4. Settlement | Customer settles 500 kWh → `EnergyReceipt` created, treasury fee deducted |
> | 5. Payout | Provider receives BLOCK in their account |
>
> **If someone disputes a reading:** A special "dispute" record is created, triggering review.

- Energy credits live in `crates/energy-market` with the node wrapper in `node/src/energy.rs`. Providers, credits, and receipts persist in sled via `SimpleDb::open_named(names::ENERGY_MARKET, …)`; set `TB_ENERGY_MARKET_DIR` to relocate the DB. The store snapshots to bytes (`EnergyMarket::{to_bytes,from_bytes}`) on every mutation and uses the same fsync+rename discipline as other `SimpleDb` consumers so restarts replay identical state.
- Oracle trust roots are defined in `config/default.toml` under `energy.provider_keys`. Each entry maps a provider ID to a 32-byte Ed25519 public key; reloads hot-swap the verifier registry via `node::energy::configure_provider_keys` so operators can rotate or revoke keys without restarts.
- RPC wiring (`node/src/rpc/energy.rs`) exposes `energy.register_provider`, `energy.market_state`, `energy.submit_reading`, `energy.settle`, `energy.receipts`, `energy.credits`, `energy.disputes`, `energy.flag_dispute`, and `energy.resolve_dispute`. The CLI (`cli/src/energy.rs`) emits the same JSON schema and prints providers, receipts, credits, and disputes so oracle adapters (`crates/oracle-adapter`) and explorers stay aligned. `docs/testnet/ENERGY_QUICKSTART.md` covers bootstrap, signature validation, dispute rehearsal, and how to script `contract-cli energy` calls.
- Governance owns `energy_min_stake`, `energy_oracle_timeout_blocks`, and `energy_slashing_rate_bps`. Proposals feed those values through the shared governance crate, latch them in `node/src/governance/params.rs`, then invoke `node::energy::set_governance_params`, so runtime hooks refresh the market config plus treasury/slashing math with no recompiles.
- Observability: `energy_market` emits gauges (`energy_provider_total`, `energy_pending_credits_total`, `energy_receipt_total`, `energy_active_disputes_total`, `energy_avg_price`), counters (`energy_provider_register_total`, `energy_meter_reading_total{provider}`, `energy_settlement_total{provider}`, `energy_treasury_fee_total`, `energy_dispute_{open,resolve}_total`, `energy_kwh_traded_total`, `energy_signature_failure_total{provider,reason}`), histograms (`energy_provider_fulfillment_ms`, `oracle_reading_latency_seconds`), and simple health probes (`node::energy::check_energy_market_health`). Feed them into the metrics-aggregator dashboards and alert whenever pending meter credits exceed the safe envelope or signature failures spike.

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

> **Plain English:**
> - **Bridges** let you move assets between The Block and other blockchains (like Ethereum). A "relayer" watches both chains and proves that a deposit on one side should unlock funds on the other.
> - **DEX (Decentralized Exchange)** lets you trade tokens without a central exchange. Order books and "trust lines" (credit relationships between parties) are tracked on-chain.
> - **HTLC (Hash Time-Locked Contracts)** enable atomic swaps: "I'll give you X if you reveal a secret; otherwise we both get refunds after timeout."

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

> **Plain English:** The gateway is the "front door" where wallets and apps talk to nodes. It handles:
> - **HTTP/API requests** — Apps call JSON-RPC methods to read state or submit transactions
> - **DNS publishing** — Register `.block` domains that point to your content
> - **Mobile cache** — Encrypted offline storage so phones work without network
> - **Light clients** — Lightweight sync for devices that can't store the full chain
>
> **User story:** Your wallet app connects to a gateway node. When you check your balance, the app calls an RPC method. When you send BLOCK, it submits a signed transaction. When you go offline, the mobile cache keeps recent data locally.

### HTTP Gateway
- `node/src/gateway/http.rs` uses `crates/httpd` for the router, TLS, and WebSocket upgrades. Gateways serve static content, APIs, and compute relays from the embedded storage pipeline.
- CLI + explorer insight commands surfaced from old `docs/gateway.md` now live in `docs/apis_and_tooling.md#gateway`.

### DNS Publishing
- DNS + `.block` records are handled by `node/src/gateway/dns.rs` with schemas archived under `docs/spec/dns_record.schema.json`.

### DNS Auctions and Staking
- Gateway domain auctions use stake-backed bids and escrowed BLOCK recorded under `node/src/gateway/dns.rs` (see `StakeEscrowRecord`). RPC/CLI support deposit, withdraw, and refund flows with error codes under the same module.

### Mobile Gateway Cache
- Mobile caches persist ChaCha20-Poly1305 encrypted blobs in sled (`node/src/gateway/mobile_cache.rs`). TTL sweeps and CLI flush commands ensure offline support without stale data.

### Light Clients
- `node/src/light_client` streams headers, DID updates, and proofs. Streaming endpoints live in `node/src/rpc/state_stream.rs` and CLI commands under `cli/src/light_sync.rs`.
- Mobile updates plus power/bandwidth heuristics from the old `docs/mobile_light_client.md` live here and in `docs/apis_and_tooling.md#light-client-streaming`.

### Read Receipts
- `node/src/gateway/read_receipt.rs` records signed acknowledgements, batches them for ledger inclusion, and exposes CLI/metrics counters. Economics for `READ_SUB` live in `docs/economics_and_governance.md`.

## Launch Governor

> **Plain English:** The launch governor is an automated system that decides when the network is "ready" for different operational phases. Think of it like a safety system that monitors network health and only enables features when metrics look stable.
>
> **Example:** Before enabling live DNS auctions, the governor watches:
> - Are blocks arriving at regular intervals?
> - Are peers staying connected?
> - Are test auctions completing successfully?
>
> Once these metrics hit target thresholds consistently (a "streak"), the governor transitions the network to the next phase.

### Gates and Actions

The governor manages two primary gates:

| Gate | Purpose | Actions |
|------|---------|---------|
| **operational** | Core network readiness | `Enter` (enable), `Exit` (disable) |
| **naming** | DNS auction readiness | `Rehearsal` (test mode), `Trade` (live auctions) |

Upcoming gates extend the same pattern:

| Gate (planned) | Scope | Notes |
|----------------|-------|-------|
| **economics** | Block reward + subsidy autopilot | Shadow mode tracks `NetworkIssuanceController` outputs (per-block rewards derived from count/volume/utilization with only the logistic miner-fairness multiplier applied), the persisted `economics_block_reward_per_block`, and `economics_prev_market_metrics` gauges. The gate flips only after those values stay within bounds for a streak and match `/wrappers` telemetry. |
| **storage**, **compute**, **energy**, **ad** | Market-specific rehearsal/live toggles | Each gate will watch the telemetry already described in the respective architecture sections (utilization, margins, disputes, backlog) and will only enable “trade” mode after sustained streaks. Backlog tracked in `AGENTS.md §15`. |

Gate states progress as: `Inactive` → `Active`/`Rehearsal` → `Trade`

### Signal Providers

The governor monitors two signal sources:

**Chain Signals** (`ChainSample`):
- `block_spacing` — Milliseconds between consecutive blocks (measures stability)
- `difficulty` — Mining difficulty trend (detects hashrate changes)
- `replay` — Success ratio of block validation replays
- `peer_liveness` — Ratio of successful peer requests vs drops
- `fee_band` — Median and P90 consumer fees

**DNS Signals** (`DnsSample`):
- `txt_success` — TXT record publish success ratio
- `dispute_share` — Ratio of auctions ending in disputes
- `completion` — Auction completion ratio
- `stake_coverage_ratio` — Locked stake vs P90 settlement amounts
- `settle_durations_ms` — How long settlements take

### Intent System

When gate conditions are met, the governor creates **intents**—timestamped records of planned state changes:

1. **Intent created** — Captures metrics snapshot, computes BLAKE3 hash
2. **Optional signing** — Ed25519 signature with node key (if `TB_GOVERNOR_SIGN=1`)
3. **Persistence** — Saved to `governor_db/` via SimpleDb
4. **Apply at epoch** — Intents apply one epoch after creation (timelock)
5. **State update** — Governance runtime receives parameter changes

Intent records include:
- `id` — Unique identifier (`{gate}-{epoch}-{seq}`)
- `params_patch` — JSON patch for governance parameters
- `snapshot_hash_hex` — BLAKE3 hash for auditability
- `metrics` — Summary and raw metrics that triggered the decision

The RPC output now also contains an `economics_prev_market_metrics` array derived from `EconomicsPrevMetric`, and `contract-cli governor status` prints this deterministic snapshot alongside the regular `economics_sample`. This makes it easy to reconcile the governor’s JSON/RPC data with the Prometheus gauges stamped `economics_prev_market_metrics_{utilization,provider_margin}_ppm`.

### Configuration

| Environment Variable | Purpose | Default/Guidance |
|---------------------|---------|---------|
| `TB_GOVERNOR_ENABLED` | Enables the background task | `false` by default; **must be `1` on shared testnets and mainnet** |
| `TB_GOVERNOR_DB` | SimpleDb path for intent history | `governor_db/` relative to node data dir |
| `TB_GOVERNOR_WINDOW_SECS` | Rolling window used for signal sampling | Default `2 ×` epoch. Increase (e.g. `4 ×`) on mainnet to avoid flapping. |
| `TB_GOVERNOR_SIGN` | Emit signed decision sidecars | `0` for local/dev. **Set to `1` on production clusters** so every intent has an Ed25519 attestation. |
| `TB_NODE_KEY_HEX` | Hex-encoded Ed25519 secret used for signing | Required when `TB_GOVERNOR_SIGN=1`. |

**Modes:** New gates should ship in **shadow mode** first—emit intents + snapshots but skip `apply_intent`—until operators confirm the metrics and thresholds behave as expected. Switch to active mode by enabling `TB_GOVERNOR_ENABLED=1` (and keeping `apply_intent` wired) only after the shadow run is documented in `docs/operations.md`.

### RPC Methods

| Method | Description |
|--------|-------------|
| `governor.status` | Current gate states, epoch, pending intents, plus the deterministic `EconomicsPrevMetric` snapshot (`economics_prev_market_metrics`) that mirrors the `economics_prev_market_metrics_{utilization,provider_margin}_ppm` gauges. |
| `governor.decisions` | Recent intent history (with `limit` param) |
| `governor.snapshot` | Load persisted decision for specific epoch |

### Source Files

- `node/src/launch_governor/mod.rs` — Gate controllers, intent planning, signal evaluation
- `node/src/governor_snapshot.rs` — Snapshot persistence and signing
- `node/src/rpc/governor.rs` — RPC handlers

## Telemetry and Instrumentation

> **Plain English:** Telemetry is how operators know what's happening inside the node. The system exports:
> - **Metrics** — Numbers like "transactions processed per second" or "peer count"
> - **Logs** — Text records of what happened and when
> - **Dashboards** — Visual graphs (via Grafana) showing health over time
>
> **The basic pattern:**
> 1. Node collects metrics internally
> 2. Metrics aggregator pulls them from multiple nodes
> 3. Grafana displays pretty graphs
> 4. Alerts fire when something looks wrong

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
- Ad targeting is now spec'd as a multi-signal platform, not a badge-only preview. `crates/ad_market` hosts the cohort schema, privacy budget manager, uplift estimator, budget broker, and attestation logic; `node/src/ad_policy_snapshot.rs`, `node/src/ad_readiness.rs`, and `node/src/read_receipt.rs` persist/snapshot selector utilization, while `node/src/rpc/ad_market.rs`, `cli/src/ad_market.rs`, `cli/src/gov.rs`, and `cli/src/explorer.rs` surface every selector knob through the RPC/CLI/explorer stack.
- **Cohort schema (`CohortKeyV2`)** — Cohorts are keyed by `{domain,String, domain_tier:DomainTier, domain_owner?:AccountId, provider?:String, badges:Vec<BadgeId>, interest_tags:Vec<InterestTagId>, presence_bucket?:PresenceBucket, selectors_version:u16}` with `DomainTier ∈ {premium,reserved,community,unverified}` sourced from `node/src/gateway/dns.rs` stakes, governance-owned interest tags, and Range Boost presence buckets. Sled keys migrate via dual writes (`cohort_v1:*` + `cohort_v2:*`) plus reversible replays inside `node/src/ad_policy_snapshot.rs`/`node/src/ad_readiness.rs` so operators can downgrade if the migration stalls.
- **Multi-signal auctions** — `crates/ad_market/src/budget.rs` teaches the budget broker to price and pace selectors individually via `{selector: SelectorBidSpec {clearing_price_usd_micros, shading_factor_bps, slot_cap, max_pacing_ppm}}`. RPC payloads (`node/src/rpc/ad_market.rs`) and CLI helpers expose selector maps for `register_campaign`, `inventory`, `distribution`, `budget`, and `readiness` so advertisers can mix badges, interest tags, domains, and presence. Explorer summaries render per-selector revenue in `cli/src/explorer.rs`.
- **Self-tuning PI controller** — Budget pacing now hinges on a PI controller that runs inside each `CampaignBudgetState`. The controller tracks the relative error between `epoch_spend` and `epoch_target`, integrates it, and applies a `dual_price` adjustment once per reservation; the error zero-crossings feed a Ziegler-Nichols inspired tuner that recalculates `Kp/Ki` so the spend stays within the configured robustness window. The tuning knobs live in `BudgetBrokerConfig.pi_tuner` (fields: `enabled`, `kp_min`, `kp_max`, `ki_min`, `ki_max`, `ki_ratio`, `tuning_sensitivity`, `zero_cross_min_interval_micros`, and `max_integral`) and are normalized alongside the existing step/dual steps. `CampaignBudgetSnapshot.pi_controller` persists the controller state so deterministic replays keep the same gain history, and the resulting `dual_price`/`kappa` traces continue to surface through the existing telemetry guards.
- **Proof-of-presence targeting** — `node/src/localnet`, `node/src/range_boost`, and `node/src/service_badge.rs` mint `PresenceReceipt {beacon_id,device_key,mesh_node,location_bucket,radius_meters,confidence_bps,minted_at_micros,expires_at_micros}` entries that `crates/ad_market/src/attestation.rs` verifies. Receipts are cached in a privacy-safe sled store, gated by governance knobs `TB_PRESENCE_TTL_SECS`, `TB_PRESENCE_RADIUS_METERS`, and `TB_PRESENCE_PROOF_CACHE_SIZE`, and exposed through new RPCs (`ad_market.list_presence_cohorts`, `ad_market.reserve_presence`). Node `bin` logic already cancels reservations when `presence_badge` checks fail; this feature extends those hooks to the new attestation types and read-readiness rehearsal gate.
- **Domain marketplace + interest ingestion** — `node/src/gateway/dns.rs` emits ownership tiers and auction/intent metadata that feed the ad-policy snapshot. A governance-owned registry maps `.block` categories and premium tiers to `interest_tags`, so advertisers can reserve or exclude those audiences. Synchronization happens alongside the ad policy snapshot pruning pipeline, and readiness snapshots surface `domain_tier_supply_ppm` and `interest_tag_supply_ppm` buckets for operators. Docs (`docs/system_reference.md`, `docs/apis_and_tooling.md`) enumerate RPC validation errors for misaligned tiers/tags.
- **Analytics, conversions, and uplift** — `crates/ad_market/src/uplift.rs` manages holdout cohorts per selector, exposing readiness/ROAS deltas via `ad_market.readiness`. `ad_market.record_conversion` accepts `value_usd_micros`, `currency_code`, `attribution_window_secs`, and `selector_weights[]` plus optional device-link attestations so advertisers can attribute conversions back to badges, interest tags, domains, and presence proofs without elevating device cohorts to first-class on-chain objects. Readiness reports publish inventory depth, presence-proof freshness histograms, domain-tier utilization, and privacy budget status per selector, while CLI/explorer commands mirror the same aggregates.
- **Privacy + governance guardrails** — `crates/ad_market/src/privacy.rs` clamps selector combinations (badge + premium domain + precise presence requires explicit opt-in) and guarantees k-anonymity before releasing supply or readiness data. Presence listing/reservation RPCs call `badge_guard` + `PrivacyBudgetManager` previews so operators see `k_anonymity_redacted`/`budget_exhausted` guardrails instead of placeholders. Violations surface via RPC errors and telemetry (`ad_privacy_budget_utilization_ratio`, `ad_privacy_denial_total`). Governance proposals (via `cli/src/gov.rs`) own selector caps, privacy budgets, interest registries, and presence TTL/radius settings.
- **Quality-adjusted pricing** — Each impression applies a cohort-quality multiplier derived from readiness streaks, presence freshness histograms, and privacy-budget headroom (see `docs/economics_and_governance.md#ad-quality-adjusted-pricing`). Multipliers are clamped by `quality_signal_{min,max}_multiplier_ppm` in `MarketplaceConfig` and exported as `ad_quality_multiplier_ppm{component}` plus readiness/freshness/privacy gauges so operators can trace why a cohort cleared high/low.
- **Tiered readiness gates** — Launch Governor evaluates per-tier readiness (contextual domain/interest vs presence-correlated) and drives separate gates (`ad_targeting_contextual`, `ad_targeting_presence`) with rehearsal→exit semantics. Intents patch `ad_rehearsal_contextual_enabled`, `ad_rehearsal_presence_enabled`, and their streak windows so rollout is tied to observed readiness streaks rather than a monolithic switch; intents/snapshots are auditable via `tb-cli governor status` and `tb-cli governor intents --gate ad_targeting_*`.
- **Resource-cost coupling** — Ad resource floors blend storage rent signals and compute-market spot prices (converted via the token oracle) with host/verifier medians, producing an explicit cost basis before clearing. Receipts persist the resulting floor breakdown for replay determinism; telemetry exposes `ad_compute_unit_price_usd_micros`, `ad_cost_basis_usd_micros{component}`, and clearing prices so operators can see compute scarcity coupling.
- **Claims + attribution** — A claims registry in `crates/ad_market` maps domain/app identities to payout addresses per role (publisher/host/hardware/verifier/liquidity/viewer). Routes are registered via `ad_market.register_claim_route`, validated against DNS ownership and optional DID anchors, persisted in marketplace metadata, and surfaced in settlement breakdowns/receipts. Ad payouts prefer these routes per role, falling back to derived viewer/host/hardware/verifier/liquidity addresses when a route is absent or invalid, so explorers/CLIs can audit where each share flowed.
- **Observability + gate cadence** — `metrics-aggregator/src/lib.rs` adds segment readiness counters (`ad_segment_ready_total{domain_tier,presence_bucket,interest_tag}`), competitiveness stats (`ad_auction_top_bid_usd_micros{selector}`, `ad_bid_shading_factor_bps{selector}`, `ad_auction_win_rate{selector}`), conversion values (`ad_conversion_value_total{selector}`), and privacy usage histograms. The aggregator exports them through `/wrappers`, Grafana panels live under `monitoring/ad_market_dashboard.json`, and `docs/operations.md#telemetry-wiring` now requires screenshots from `npm ci --prefix monitoring && make monitor` whenever these metrics change. Every touch to `crates/ad_market`, `node/src/rpc/ad_market.rs`, `node/src/localnet`, `node/src/range_boost`, `node/src/gateway/dns.rs`, `node/src/ad_policy_snapshot.rs`, `node/src/ad_readiness.rs`, `metrics-aggregator/`, `monitoring/`, or the associated CLI/explorer files must rerun the full gate list (`just lint`, `just fmt`, `just test-fast`, `just test-full`, `cargo test -p the_block --test replay`, `cargo test -p the_block --test settlement_audit --release`, `scripts/fuzz_coverage.sh`) with transcripts attached per `AGENTS.md §0.6`.

### Law-enforcement Portal and Jurisdiction Packs
- LE logging (`node/src/le_portal.rs`) records requests, actions, canaries, and evidence logs, with privacy redaction optional. Jurisdiction packs (`jurisdiction/`, `docs/security_and_privacy.md#jurisdiction-packs`) scope consent defaults and audit hooks.

### Range-Boost and LocalNet Telemetry
- Mesh queue depth, hop latency, and fault toggles are exported via `node/src/range_boost` metrics. Operators manage peers and mesh policies through the CLI + `docs/operations.md#range-boost`.
