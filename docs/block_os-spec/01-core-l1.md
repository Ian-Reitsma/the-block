# Core L1 — Consensus, Sharding, and Fees

## 1. Consensus Layer
### Canonical Modules
- `node/src/consensus/` — PoW driver (`pow.rs`), PoH tick generator (`poh.rs`), fork-choice hints, and QUIC handshake policy (`transport_quic.rs`).
- `node/src/blockchain/` — Block processing pipeline, inter-shard fork choice, macro-block checkpoints, and difficulty retunes. `process.rs` is the entry point invoked by RPC and CLI handlers.
- `ledger/` — Token registry plus BLOCK emission math. `ledger/src/token.rs` exposes `TokenRegistry` and `Emission` used when assembling coinbase outputs.
- `state/` — sled-backed trie providing Merkle proofs for consensus snapshots.

### State Structures & Storage Keys
| Structure | Definition | Storage |
| --- | --- | --- |
| `consensus::pow::BlockHeader` | Header template with PoH checkpoints, BLOCK base fee, VDF commitments, and `l2_roots`. | Serialized into `block_binary.rs` and persisted under the `blocks` column family in `node/src/storage`. |
| `blockchain::snapshot::SnapshotManager` | Persists state/ledger snapshots to `state/snapshots/<height>` alongside diff files. Keeps consumer/industrial BLOCK balances plus nonces per account. | Filesystem snapshots + sled column families keyed by account hash. |
| `ledger::shard::ShardState` | Shard IDs + state roots. | Stored per `shard:{id}` column family key `state`. |

### RPC & CLI Surfaces
- `node/src/rpc/consensus.rs::difficulty` → JSON-RPC method `consensus.difficulty`. Returns current PoW target for monitoring.
- `node/src/rpc/state_stream.rs` → long-poll snapshots for light-clients (used by CLI `contract-cli light stream`).
- `node/src/rpc/governor.rs` + `node/src/launch_governor` → exposes checkpoint and macro-block metadata consumed by CLI `contract-cli gov checkpoints`.
- `cli/src/node.rs` → `contract-cli node status` displays consensus height, PoH tick health, and last macro-block hash.

### Message / Transaction Types
- `node/src/transaction.rs` defines `TransactionPayload` variants (transfer, storage proof, governance). Each variant maps to a fee lane.
- `node/src/transaction/fees.rs` applies base fee + lane multipliers derived from the consensus header’s `base_fee`.
- `node/src/mempool` tags pending transactions by `FeeLane` to enforce QoS while consensus pulls transactions out of each lane per block.

### Settlement Flow
1. Gossip (`node/src/gossip`) propagates candidate blocks + transactions using adaptive fanout from AGENTS spec.
2. `consensus::pow::Miner::mine` seals blocks with base fee + PoH checkpoint hash.
3. `blockchain::process::apply_block` mutates sled-backed account trie, snapshotting via `SnapshotManager` once `interval` ticks elapse.
4. Coinbase assembly (`node/src/treasury_executor.rs`) credits BLOCK to miners plus subsidy buckets (`STORAGE_SUB`, `READ_SUB`, `COMPUTE_SUB`).
5. Macro-block checkpoints (`node/src/blockchain/macro_block.rs`) anchor shard roots + proof receipts for light-clients.

## 2. Sharding Implementation
### Modules & Responsibilities
- `node/src/blockchain/inter_shard.rs` — Cross-shard queue, deterministic ordering, and TTL enforcement.
- `node/src/blockchain/shard_fork_choice.rs` — Maintains per-shard heads and reconciles with macro-block checkpoints.
- `node/src/storage/blob_chain.rs` + `node/src/blob_chain.rs` — Canonical micro-shard root assembly (RootAssembler) and blob root scheduling referenced in AGENTS.
- `ledger/src/shard.rs` — Defines `ShardState` serialization (`ShardState::to_bytes`, `::from_bytes`).
- `node/src/gateway/storage_alloc.rs` — Schedules shard placements for storage workloads; same weighting is reused for compute lanes.

### State Structures
| Structure | Description | Storage |
| --- | --- | --- |
| `blockchain::inter_shard::PendingMessage` | Envelope with origin shard, destination shard, commitment hash, and expiry height. | Stored in sled tree `inter_shard:<dst>` keyed by message index. |
| `state::MerkleTrie` roots | Each shard column family maintains its own trie root hashed into macro-block checkpoints. | Column family `shard:{id}`. |
| `node/src/root_assembler.rs::RootBundle` | Canonical micro-shard bundle: slot, size class, hash, and ordered `MicroShardRootEntry` items (hash, shard, lane, payload bytes, DA window). | Stored via `RootManifest` entries keyed by size class + slot and hashed into block headers. |

### RPC
- `node/src/rpc/shards.rs` (generated via `rpc/state_stream.rs`) — Allows clients to subscribe to shard roots; used by `contract-cli light follow-shard`.
- `node/src/rpc/storage.rs::manifest_summaries` — surfaces storage manifests along with coding algorithm selection; indirectly documents which shards hold data.
- `node/src/rpc/ledger.rs::shard_balances` — exposes per-shard BLOCK balances for auditing.

### Cross-Shard Settlement
1. Each shard emits `ShardState` root + pending message commitments.
2. Macro-block checkpoints collect shard roots + commitments; `inter_shard.rs` drives replay.
3. Receivers pick up messages, execute them against shard state, and emit receipts recorded in `node/src/blockchain/privacy.rs` for audit.

## 3. Fee Mechanism
### Modules
- `node/src/fee` and `node/src/fees.rs` — Helpers for base-fee, subsidy, and QoS adjustments.
- `node/src/mempool/qos.rs` — Slot accounting per `FeeLane`.
- `node/src/transaction/fees.rs` — Probability-weighted fee escalators.
- `node/src/treasury_executor.rs` — Split payments between miner, treasury (`governance`), and subsidy ledgers.

### State + Keys
| Item | Description |
| --- | --- |
| `FeeLane` | Enum defined in `node/src/transaction.rs`, values `Consumer`, `Industrial`, `Governance`, plus read receipts. Used in mempool + settlement. |
| `TreasurySchedule` | In `node/src/treasury_executor.rs`, enumerates BLOCK disbursement schedule stored in sled tree `treasury:schedules`. |
| `SubsidyLedger` | Maintained in `node/src/ledger_binary.rs`, keyed by `subsidy:<bucket>` (STORAGE/READ/COMPUTE). |

### RPC/CLI
- `node/src/rpc/fees.rs::base_fee` → `fees.base_fee` method; CLI `contract-cli fees show`.
- `node/src/rpc/treasury.rs` → `treasury.status`, `treasury.streams`. CLI `contract-cli treasury list` ties into the same JSON-RPC payloads.
- `node/src/rpc/storage.rs::upload` uses fee hints when computing per-block payments; `cli storage upload` relays base fee + EIP-1559 style tips.

### Settlement Flow
1. Transactions include `FeeLane` + desired tip. `node/src/mempool` orders by lane-specific QoS windows.
2. Consensus block assembly merges lanes until QoS budget is met.
3. `node/src/transaction/fees.rs` calculates burn + distribution; ledger updates are mirrored in `state::MerkleTrie`.
4. Treasury executor receives 5% cut (configurable via governance). `ledger` updates minted BLOCK and subsidy buckets recorded in `metrics-aggregator` via gauges `ledger_subsidy_bucket_total`.
5. RPC surfaces final balances via `ledger.balance` and `governance.treasury_status`. CLI/lite clients rely on the same endpoints.

## References & Tests
- Deterministic replay validated in `node/tests/replay.rs` and `sim/` harnesses.
- Snapshot encoding/decoding covered by `state/tests/snapshot.rs`.
- Fee QoS invariants tested in `node/tests/mempool_fee.rs`.
- Follow AGENTS.md when adjusting retune constants, base-fee parameters, or shard scheduling.
