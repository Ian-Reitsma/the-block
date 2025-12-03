# Service Credits — Storage, Compute, Bandwidth

## 1. Shared Concepts
- **Unified CT ledger** — Subsidies map to the CT `TokenRegistry` (see `docs/economics_and_governance.md#ct-supply-and-sub-ledgers`). Snapshot data lives under `state/` using the same sled Merkle trie as consumer balances.
- **Buckets** — `STORAGE_SUB_CT`, `READ_SUB_CT`, `COMPUTE_SUB_CT` maintained via `node/src/treasury_executor.rs` and surfaced in RPC `treasury.status`.
- **Read receipts** — `node/src/read_receipt.rs` batches gateway reads with auditing metadata; CLI/regressions covered in `docs/architecture.md#gateway-and-client-access`.

## 2. Storage Credits
### Modules
- `storage_market/` — Contract & replica state persisted through sled `Engine`. `ReplicaIncentive` tracks deposits, EWMA proof scores, and block numbers.
- `node/src/storage` — Upload/repair flows, manifest metadata, proof verification.
- `node/src/rpc/storage.rs` — JSON-RPC handlers: `storage.upload`, `storage.challenge`, `storage.incentives`, `storage.provider_profiles`, `storage.manifest_summaries`, `storage.repair_run`, `storage.repair_chunk`, `storage.set_provider_maintenance`.
- `cli/src/storage.rs` — `tb-cli storage upload|challenge|providers` map 1:1 to the RPC functions.

### State Structures
| Item | Description |
| --- | --- |
| `StorageContract` (`storage/src/contract.rs`) | Contains `object_id`, `shares`, `price_per_block`, coding metadata, and retention windows. Serialized using `foundation_serialization`. |
| `ReplicaIncentive` (`storage_market/src/lib.rs`) | Holds provider ID, shares, deposit, proof stats. Stored in sled tree `market/contracts`. |
| `ProofRecord` | Emitted when `record_proof_outcome` runs. Used by RPC `storage.challenge`. |

### Settlement Flow
1. Clients call `storage.upload` via RPC. The gateway uses `storage_alloc::allocate` to choose providers.
2. `storage_market::StorageMarket::register_contract` calculates deposits and writes `ContractRecord`.
3. Retrieval challenges hit `storage.challenge`, which verifies proofs via `StorageContract::verify_proof`. Success increments EWMA counters; failure slashes deposits and decreases scheduler reputation via `compute_market::scheduler::merge_reputation`.
4. Payments accrue as CT in contract state, later withdrawn when contract expires. Ledger entries reference `object_id` for audit.
5. Telemetry increments `STORAGE_CONTRACT_CREATED_TOTAL`, `RETRIEVAL_SUCCESS_TOTAL`, `RETRIEVAL_FAILURE_TOTAL` in `node/src/telemetry.rs`.

## 3. Compute Credits
### Modules
- `node/src/compute_market/` — Scheduler, matcher, receipts, settlement engine, SNARK bundle cache, pricing board.
- `node/src/rpc/compute_market.rs` — Methods: `compute.stats`, `compute.scheduler_metrics`, `compute.scheduler_stats`, `compute.reputation_get`, `compute.job_requirements`, `compute.job_cancel`, `compute.provider_hardware`, `compute.settlement_audit`, `compute.provider_balances`, `compute.recent_roots`, `compute.sla_history`.
- `cli/src/compute.rs` — Subcommands `tb-cli compute submit|cancel|providers|settlements` mirroring RPC.

### State Structures
| Item | Description |
| --- | --- |
| `scheduler::PendingJob` | Job envelope with `job_id`, `priority`, `lane`, `effective_priority`. Stored in `scheduler::Queues` keyed by lane.
| `matcher::Receipt` | Records buyer, provider, price, issued block, and lane label. Persisted in receipt store + surfaced over RPC `recent_roots`.
| `settlement::Settlement` | Contains job/receipt IDs, payout CT, proofs, and audit metadata. Stored in sled tree `compute:settlements`. |
| `price_board::Quote` | Maintains EWMA pricing windows per lane, inform `compute.stats` output. |

### Settlement Flow
1. Buyers post jobs through RPC `compute.submit_job` (handler in `node/src/rpc/compute_market.rs`). Jobs enter lane-specific queues.
2. Matcher rotates lanes (fairness window) and pairs jobs to providers; `Receipt` is emitted and sent to `ReceiptStore`.
3. Providers submit SNARK proofs; `settlement::engine` validates and issues `Settlement` records.
4. Treasury executor credits CT to provider accounts; `metrics-aggregator` collects `compute_market.sla_history` for dashboards.
5. CLI/Explorer surfaces receipts + payouts for audit.

## 4. Bandwidth / Gateway Credits
### Modules
- `node/src/gateway` — Read path, rate limiting, cached telemetry.
- `gateway/` services (Rust) handle LocalNet and Range Boost logic (see `docs/architecture.md#localnet-and-range-boost`).
- `node/src/rpc/gateway.rs` (via `gateway/read_receipt.rs`) — Methods `gateway.reads_since`, `gateway.cache_status`, `gateway.flush_cache`.
- `cli/src/gateway.rs` — `tb-cli gateway cache|reads` commands.

### State
| Item | Description |
| --- | --- |
| `read_receipt::Batch` | Aggregated read events persisted under `gateway:reads` sled tree and exported in telemetry metric `gateway_reads_since`. |
| `mobile_cache` sled DB | Lives in `mobile_cache/`, storing ChaCha20-Poly1305 encrypted offline responses keyed by request hash. Keys derived from `TB_MOBILE_CACHE_KEY_HEX` fallback to `TB_NODE_KEY_HEX`. |

### Settlement Flow
1. Reads accrue in batches via `read_receipt::Batcher`. Each batch includes jurisdiction + policy metadata for billing.
2. Billing pipeline converts bytes → CT debits from `READ_SUB_CT`. `gateway` RPC exposes stats so explorers and the CLI can reconcile.
3. Governance policy toggles (jurisdiction packs, fee multipliers) determine whether reads stay free or pull from rebates. Handled in `node/src/gateway/policy.rs`.

## 5. Interfaces & Testing
- **RPC tests** — `node/tests/rpc_storage.rs`, `node/tests/rpc_compute.rs`, `gateway/tests/read_receipts.rs`.
- **CLI smoke** — `cli/tests/storage.rs`, `cli/tests/compute.rs` cover request/response shape.
- **Telemetry** — `monitoring/tests/dashboard.rs` ensures the metrics referenced above (storage/compute/gateway) stay visible.

Use these patterns when building the energy-credit vertical in `06-physical-resource-layer.md`.
