# Governance & Treasury Specification

## 1. Canonical Modules
- `governance/` crate — Houses `GovStore`, proposal DAG validation, Kalman retune helpers, release approvals, and treasury accounting logic. Modules include `bicameral.rs`, `params.rs`, `proposals.rs`, `treasury.rs`, and `store.rs`.
- `node/src/governance/` — Runtime wiring, telemetry exports, RPC bindings.
- `node/src/treasury_executor.rs` — Runtime executor that reads disbursement intents from `GovStore` and applies them to on-chain balances.
- `node/src/rpc/governance.rs` & `node/src/rpc/treasury.rs` — JSON-RPC entry points for CLI/Explorer.
- `cli/src/governance.rs` & `cli/src/treasury.rs` — CLI commands bridging RPC.

## 2. State & Storage
| Structure | Path | Description |
| --- | --- | --- |
| `governance::store::GovStore` | `governance/src/store.rs` | sled database (`governance.db/`) storing proposals, votes, executor leases, treasury snapshots, release DAG, DID revocations. Keys follow prefixes `proposal:<id>`, `vote:<proposal_id>:<badge_id>`, `treasury:intent:<nonce>`. |
| `governance::Proposal` | `governance/src/proposals.rs` | Contains proposal metadata, payload (param updates, runtime/transport/storage policies, release approvals). Serialized via `foundation_serialization`. |
| `governance::Vote` | `governance/src/proposals.rs` | Records badge ID, chamber (Operator/Builder), choice, signature. Stored under `vote:` prefix. |
| `governance::treasury::TreasuryDisbursement` | `governance/src/treasury.rs` | Amounts + destination accounts derived from BLOCK subsidy ledger. Settled by `TreasuryExecutor`. |
| `governance::store::TreasuryExecutorSnapshot` | Captures executor lease + nonce, visible via RPC for monitoring. |

## 3. RPC & CLI
| Method | File | Description |
| --- | --- | --- |
| `governance.submit_proposal` | `node/src/rpc/governance.rs` | Accepts serialized `Proposal` JSON, persists via `GovStore::create_proposal`. CLI: `contract-cli gov submit`. |
| `governance.proposal_status` | same | Returns vote counts, quorum status, activation/rollback windows. CLI: `contract-cli gov show --proposal <id>`. |
| `governance.vote` | same | Records votes; CLI `contract-cli gov vote`. |
| `governance.param_history` | same | Streams parameter changes since height. Used by monitoring + docs. |
| `treasury.status` | `node/src/rpc/treasury.rs` | Returns `TreasuryExecutorSnapshot`, consumer/industrial balances, pending disbursements. CLI `contract-cli treasury status`. |
| `treasury.disbursements` | same | Lists staged/approved payouts. CLI `contract-cli treasury list`. |

## 4. Proposal Lifecycle
1. **Authoring** — CLI `contract-cli gov submit --file proposal.json` calls `governance.submit_proposal`. JSON follows `governance::Proposal` schema (payload type defined in `governance/src/proposals.rs`).
2. **Snapshot** — Voting snapshots (badge sets) read from `node/src/service_badge.rs`. Snapshots stored in `GovStore` under `snapshot:<proposal_id>`.
3. **Voting** — Operators and Builders submit votes via RPC `governance.vote`, signed using CLI keystore. Votes stored with chamber label, enabling bicameral thresholds enforced by `governance::bicameral::tally`.
4. **Activation** — After quorum, proposals enter timelock defined by `governance::store::ACTIVATION_DELAY`. `GovStore` writes `proposal:<id>:status=Approved` and queues actions (param updates, disbursements, runtime/transport/storage policies) for execution.
5. **Execution** — `node/src/treasury_executor.rs` polls `GovStore::load_execution_intents`, obtains lease (`refresh_executor_lease`), signs via configured signer, and submits to ledger. Execution receipts recorded under `treasury:history`. Rollback window enforced via `ROLLBACK_WINDOW_EPOCHS`.
6. **Telemetry** — `governance` module increments `governance_proposal_total`, `governance_vote_total`, and `treasury_disbursement_total` metrics. Dashboard references defined under `monitoring/src/dashboard.rs`.

## 5. Treasury Flow
- Income: BLOCK from base fee burns diverted to treasury per `node/src/treasury_executor.rs`. Balances are tracked as a single BLOCK ledger (see `governance::store::TreasuryBalances`).
- Outgoing: Approved disbursements specify `account_id`, `amount`, `memo`. Executor enforces `nonce_floor` guard and logs to `treasury_balances` history for explorer consumption.
- Coinbase integration: `node/src/treasury_executor.rs` hooks into block production so minted BLOCK honors the latest governance-selected treasury split.

## 6. Governance + Energy Market Integration Hooks
- **Proposal Type Extension** — Add `ProposalPayload::UpdateEnergyMarketParams` to `governance/src/proposals.rs` once energy crate lands. Documented in `06-physical-resource-layer.md`.
- **Treasury Policy** — Use `governance::params::decode_storage_engine_policy` patterns to parse new energy policy objects (jurisdictional fees, oracle bond requirements).
- **Executor Dependency Check** — Implement `dependency_check` closure (see `TreasuryExecutorConfig`) to ensure energy disbursements reference fresh oracle receipts.

## 7. Testing & Tooling
- `governance/tests/*.rs` covers proposal DAG validation, treasury balance math, and serialization. Extend with new proposal types.
- `cli/tests/governance.rs` ensures CLI command/response schema alignment.
- `tools/gov_graph` renders DAG + timelock timelines for explorer/testnet docs.

Always cross-reference AGENTS.md before editing governance or treasury logic.
