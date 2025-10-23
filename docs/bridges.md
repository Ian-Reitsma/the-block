# Bridge Primitives and Light-Client Workflow
> **Review (2025-09-25):** Synced Bridge Primitives and Light-Client Workflow guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The bridge subsystem moves value between The‑Block and external chains without introducing custodial risk. This document describes the lock/unlock implementation, light‑client header verification, relayer proof format, CLI flows, and outstanding work.

## Architecture Overview

1. **Lock Phase**
 - Users invoke `blockctl bridge deposit --amount <amt> --dest <chain>`.
  - The transaction locks funds in the on-chain `Bridge` contract and emits an event containing the deposit ID, destination chain, and current partition marker so downstream relayers can avoid isolated shards.
2. **Relayer Proof**
   - Off-chain relayers watch the event stream and submit a Merkle proof to the destination chain.
   - Proofs include the deposit ID, amount, source account, and the BLAKE3 commitment of the lock event.
3. **Unlock Phase**
   - Once the destination chain verifies the proof, relayers call `blockctl bridge withdraw --id <deposit-id>` on The‑Block.
   - The contract validates that the deposit is unspent and releases the locked balance to the caller.

All bridge state lives under the `SimpleDb` tree (`state/bridges/`) so channel balances, relayer sets, and pending withdrawals survive restarts and reload automatically.

## Light-Client Header Verification

`verify_header` validates external chain headers and Merkle proofs before minting mirrored tokens.

```rust
struct Header {
    chain_id: String,
    height: u64,
    merkle_root: [u8;32],
    signature: [u8;32], // crypto_suite::hashing::blake3::hash(chain_id || height || merkle_root)
}

struct Proof {
    leaf: [u8;32],
    path: Vec<[u8;32]>,
}
```

Sequence:

1. Relayers fetch an external `Header` and Merkle `Proof` for the deposit event.
2. `blockctl bridge deposit --header header.json --proof proof.json` calls the `bridge.verify_deposit` RPC which forwards to `Bridge::deposit_with_relayer`.
3. `deposit_with_relayer` invokes `verify_pow` and `light_client::verify`, credits the user on success, and persists the full header JSON under `state/bridge_headers/<hash>.json` to prevent replay and allow audit.
4. Telemetry counters `bridge_proof_verify_success_total` and `bridge_proof_verify_failure_total` track verification results.

Sample `header.json` and `proof.json` files reside in `examples/bridges/` for development testing.

The `state/bridge_headers/` directory stores one file per verified header. Each
entry contains the serialised `Header` plus the block height that introduced it.
Schema migration details live in
[`docs/schema_migrations/v8_bridge_headers.md`](schema_migrations/v8_bridge_headers.md).

## Relayer Proof Format

```text
struct LockProof {
    deposit_id: u64,
    amount: u64,
    source: [u8; 32],
    dest_chain: u16,
    merkle_path: Vec<[u8;32]>,
}
```

Relayers must sign the serialized `LockProof` with their Ed25519 key. The contract verifies:

- signature matches a whitelisted relayer,
- `deposit_id` exists and is still locked,
- Merkle path recomputes the event root.

| Field       | Type        | Example file |
|-------------|-------------|--------------|
| `deposit_id`| `u64`       | `examples/bridges/proof.json` |
| `amount`    | `u64`       | `examples/bridges/proof.json` |
| `source`    | `[u8;32]`   | `examples/bridges/header.json` |
| `dest_chain`| `u16`       | `examples/bridges/proof.json` |
| `merkle_path`| `Vec<[u8;32]>` | `examples/bridges/proof.json` |

## Relayer Workflow & Incentives

Relayers post governance-controlled collateral (`BridgeIncentiveParameters`) before the node will assign any duty. Deposits still require a quorum of signatures: the `bridge.verify_deposit` RPC accepts a `RelayerBundle`, recomputes every proof, and enforces that at least `BridgeConfig::relayer_quorum` entries verify against the persisted shard affinity. Each duty emits a structured `DutyRecord` that captures the relayer roster, reward/penalty amounts, timestamps, and failure reasons so the entire lifecycle is auditable.

1. `PowHeader` wraps an external header with a lightweight PoW target. `verify_deposit` refuses headers that fail `verify_pow`, credits the active relayer with the configured `duty_reward`, and appends the duty to the sled-backed ledger.
2. Invalid proofs debit the signer’s bond by `failure_slash`, increment `bridge_invalid_proof_total`, and mark the duty failed. Challenge wins slash *every* signer recorded on the bundle via `challenge_slash` and link the duty outcome to the challenger.
3. External settlement proofs (documented below) create an additional `DutyKind::Settlement` entry so operators can track who supplied the attestation and when governance requirements were satisfied.

The bridge store persists incentive parameters and per-relayer accounting snapshots alongside channel state:

- `BridgeIncentiveParameters` tracks `min_bond`, `duty_reward`, `failure_slash`, `challenge_slash`, and `duty_window_secs`. Governance proposals update these keys atomically; the node refreshes them on every deposit/withdrawal path. The corresponding runtime parameter keys remain `bridge_min_bond`, `bridge_duty_reward`, `bridge_failure_slash`, `bridge_challenge_slash`, and `bridge_duty_window_secs`.
- `RelayerAccounting` records each relayer’s bond, cumulative rewards, pending balances, claimed totals, penalties, and duty counters. The ledger is persisted under the bridge store and exposed through `bridge.relayer_accounting` (RPC) or `blockctl bridge accounting` (CLI).
- Duty assignments are stored as `DutyRecord` entries (`Pending`, `Completed`, `Failed`, or `Settlement`). Operators can query the log via `bridge.duty_log` / `blockctl bridge duties` with optional filters and limits.
- Challenge and finalize flows update the duty store in place, ensuring every reward or slash is backed by a recorded duty outcome. Integration coverage in `node/tests/bridge_incentives.rs` simulates honest/faulty relayers, settlement proofs, and dispute escalations to verify the accounting end-to-end.

### Governance Reward Claims

Governance now mints reward approvals that relayers redeem on-demand. Authorizations are stored as `RewardClaimApproval` records in the sled-backed governance store (`GovStore::record_reward_claim`) and surfaced via `bridge.reward_claims` / `blockctl bridge reward-claims` for audit. Requests accept optional `cursor`/`limit` parameters and return both the current page and the `next_cursor`, allowing operators to stream large histories without materialising the full retention window. A relayer redeems an approval by issuing `blockctl bridge claim <relayer> <amount> <approval-key>`, which forwards to `bridge.claim_rewards`. The node consults governance via `ensure_reward_claim_authorized`, decrements the remaining allowance, and persists a signed `RewardClaimRecord` with monotonic IDs so operators can reconcile payouts.

The `node/tests/bridge_incentives.rs::reward_claim_requires_governance_approval` scenario covers success paths, duplicate prevention, allowance exhaustion, and storage persistence. Additional unit tests in `governance/src/store.rs` and `node/src/governance/store.rs` exercise the sled helpers to guarantee approvals survive reopen and reject mismatched relayers.

When telemetry is enabled the node records `BRIDGE_REWARD_CLAIMS_TOTAL` and `BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL` for each approved payout, `BRIDGE_SETTLEMENT_RESULTS_TOTAL{result,reason}` for every settlement submission, and `BRIDGE_DISPUTE_OUTCOMES_TOTAL{kind,outcome}` whenever a withdrawal or settlement duty resolves. Dashboards can therefore track how many approvals have been consumed, which settlement errors appear, and how disputes resolve across relayer cohorts. The updated `node/tests/bridge_incentives.rs::telemetry_tracks_bridge_flows` case keeps these counters wired up under the `test-telemetry` feature.

### Reward Accrual Ledger

Every completed duty now emits a `RewardAccrualRecord` alongside the existing accounting snapshots. The sled-backed ledger captures the duty kind (deposit, withdrawal, settlement), the asset and user, the bundle roster, the recorded reward amount, and any associated commitment/proof metadata. Operators can paginate the history with `bridge.reward_accruals` / `blockctl bridge reward-accruals`, optionally filtering by relayer or asset while receiving a `next_cursor` token for streaming dashboards. The new CLI integration test (`cli/tests/bridge.rs::bridge_reward_accruals_paginates_requests`) validates the JSON-RPC payloads, and `node/tests/bridge_incentives.rs` asserts that deposits, settlement submissions, and finalised withdrawals append the expected entries with monotonic IDs.

### Settlement Proofs and External Releases

Channels may opt into external settlement attestation by toggling `requires_settlement_proof`. When enabled, each withdrawal produces a `DutyKind::Settlement` entry that remains pending until a relayer submits an `ExternalSettlementProof` via `bridge.submit_settlement`. Proofs must include a digest computed by `bridge_types::settlement_proof_digest(asset, commitment, chain, height, user, amount, relayers)` so governance can re-derive the attestation deterministically; mismatched hashes surface as `BridgeError::SettlementProofHashMismatch`. The node also records a per-asset, per-chain height watermark and rejects stale submissions with `BridgeError::SettlementProofHeightReplay` before updating the settlement log. Fingerprints still guard against replay, the full metadata lands in `BridgeState::settlement_log`, and any outstanding dispute flags are cleared. Operators can inspect the history through `bridge.settlement_log` / `blockctl bridge settlement-log` filtered by asset, with `cursor`/`limit` controls and a `next_cursor` response for streaming dashboards.

`blockctl bridge configure` now supports partial updates for channel settings: unspecified fields leave the current configuration intact, `--requires-settlement-proof` toggles proof enforcement without clobbering other values, and `--clear-settlement-chain true` removes any previously configured chain label. The RPC surfaces the same behaviour, allowing declarative updates via automation.

### Dispute Audit & Operator Reporting

Every pending withdrawal feeds into the dispute auditor, which now summarises challenge state, deadlines, settlement requirements, and per-relayer outcomes. The `bridge.dispute_audit` RPC (and `blockctl bridge dispute-audit`) renders these summaries per-asset with cursor-based pagination so governance dashboards can visualise expiring duties, challengers, settlement submissions, and unresolved disputes without downloading the entire deque. The auditor cross-links the relevant duty IDs and settlement fingerprints to simplify forensic reviews.

## CLI Examples

Lock funds on The‑Block using a light-client proof:

```bash
blockctl bridge deposit \
  --user alice \
  --amount 50 \
  --header header.json \
  --proof proof.json
```

After the lock is observed and proven on Ethereum, unlock back on The‑Block using a multi-relayer proof bundle. Withdrawals enter a challenge window; provide the relayer list up front and monitor the returned commitment:

```bash
blockctl bridge withdraw \
  --user alice \
  --amount 50 \
  --relayers r1,r2
```

If a challenge is required, submit it with the commitment hash returned by the CLI:

```bash
blockctl bridge challenge --commitment <hex>
```

Operators can also monitor the live bridge ledger via:

```bash
blockctl bridge pending --asset native
blockctl bridge challenges
blockctl bridge relayers --asset native
blockctl bridge accounting --asset native
blockctl bridge duties --asset native --limit 20
blockctl bridge history --asset native --limit 20
blockctl bridge slash-log
blockctl bridge reward-claims --relayer r1
blockctl bridge reward-accruals --asset native --relayer r1
blockctl bridge settlement-log --asset native
blockctl bridge dispute-audit --asset native
blockctl bridge assets
blockctl bridge configure native --relayer-quorum 3 --requires-settlement-proof true
```

### Asset Supply Snapshots

The bridge now tracks per-asset balances for both native collateral and wrapped
issuance. The `bridge.assets` RPC (and `blockctl bridge assets`) returns records
with the following shape:

| Field     | Description |
|-----------|-------------|
| `symbol`  | Asset identifier |
| `locked`  | Total native tokens currently escrowed on The‑Block |
| `minted`  | Outstanding wrapped supply minted for the external chain |
| `emission`| Emission configuration (`fixed` or `linear`) |

The response includes every registered or recently active asset so operators can
compare native collateral against outstanding wrapped issuance at a glance.

Relayer bonds can be provisioned off-chain and topped up through the RPC by calling
`blockctl bridge bond --relayer <id> --amount <tokens>`; the accounting view immediately reflects the new collateral once the transaction is finalised.

Reward approvals are redeemed via `blockctl bridge claim <relayer> <amount> <approval-key>`. Channels that require external attestation accept settlement submissions through `blockctl bridge settlement --asset native --relayer r1 --commitment <hex> --settlement-chain l1 --proof-hash <hex> --height <block>`; the resulting records appear instantly in `bridge.settlement_log` and the dispute auditor output.

`header.json` and `proof.json` follow the formats above and are consumed directly by the CLI.

## Outstanding Work

- **Cross-Domain Treasury Sweeps** – finalise multi-asset treasury flows that stream reward accrual deltas into governance reports and expose cumulative views through the CLI.
- **Remote Proof Sampling** – add offline verification hooks so operators can recompute settlement digests from archived L1 headers without replaying the entire bridge database.
- **Relayer Incentive Analytics** – extend the monitoring row with accrual rate histograms and duty completion percentile panels to spotlight lagging relayers before governance intervention.

Progress: 97.8%

## Dispute Resolution & Threat Model

The node now persists per-asset bridge channels in a sled-backed database so that lock
balances, pending withdrawals, and relayer collateral all survive process restarts and
chain rollbacks. Deposits record their originating proof metadata with monotonically
increasing nonces to prevent replay; receipts can be paged via `bridge.deposit_history`
and exported for audit.

Withdrawals enter a challenge window (default 30 seconds) where any operator can invoke
`bridge.challenge_withdrawal`. Challenged releases immediately re-credit the user’s
locked balance, mark the receipt for review, and slash every relayer that signed the
bundle. Collateral is debited from the bond ledger and the slashing event appears under
`bridge.slash_log`. Successful releases require a governance attestation: the node calls
`governance::ensure_release_authorized("bridge:<asset>:<commitment>")` before honouring a
withdrawal, guaranteeing that signer thresholds are enforced alongside the relayer quorum.

Telemetry counters `bridge_challenges_total` and `bridge_slashes_total` expose these
events for dashboards, while CLI helpers allow operators to enumerate active challenges
and relayer quorum composition. The threat model assumes at least one honest challenger
per channel during the dispute window; even if a malicious quorum attempts to withdraw
forged funds, bonded relayers are penalised and the audited receipts provide clear
evidence for governance intervention.
