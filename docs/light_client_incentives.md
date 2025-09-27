# Light-Client Incentives Playbook
> **Review (2025-09-25):** Synced Light-Client Incentives Playbook guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Light-client relayers earn consumer-token (CT) rebates whenever they submit
valid proofs that are later folded into mined blocks. This guide explains the
operational workflow, observability surface, and governance controls that keep
rebate accounting durable across restarts and chain reorgs.

## Data Persistence and Auditing

* **Durable ledger** – Rebates accumulate inside the node's
  `light_client/proof_rebates` store. Each mined block seals a claim receipt
  keyed by block height so duplicate claims are prevented even if the node
  restarts.
* **Per-relayer metadata** – The tracker records total proofs relayed,
  outstanding balances, cumulative CT claimed, and the last claim height for
  every relayer ID. The `light_client.rebate_status` RPC (and matching CLI
  command) exposes these snapshots for monitoring.
* **History inspection** – Receipts are paginated through the
  `light_client.rebate_history` RPC and the `contract light-client rebate-history`
  CLI command. Both endpoints accept optional relayer filters and cursors so
  operators can audit payouts without scanning the entire dataset.
* **Explorer surfaces** – The explorer API publishes
  `/light_client/top_relayers` and `/light_client/rebate_history` routes to drive
  dashboards showing the most active relayers and the latest payouts.
* **Telemetry** – Prometheus counters `PROOF_REBATES_PENDING_TOTAL` and
  `PROOF_REBATES_CLAIMED_TOTAL` expose outstanding balances and realised payouts;
  dashboards should alert on unexpected growth or drops in these gauges.

## Claim Cadence

Relayers should coordinate with miners to claim rebates shortly before block
production so pending balances are flushed into the next coinbase:

1. Monitor pending balances via `contract light-client rebate-status` or the
   JSON RPC endpoint.
2. Submit proofs continuously; the node aggregates them automatically.
3. When pending balances are non-zero, trigger block production (or wait for the
   next scheduled block). The miner will call `claim_all(height)` internally and
   append the rebate receipt to the block coinbase.
4. Verify the resulting receipt through the history RPC to confirm the payout
   was sealed.

If a relayer needs to reconcile across multiple nodes, use the history pagination
cursor to export receipts in deterministic height order.

## Reorg and Restart Safety

* **Automatic rollback** – During a chain reorg the node replays claim receipts
  in reverse height order. Any reverted blocks have their rebate amounts restored
  to the pending pool so the relayer can be paid once the canonical chain is
  known.
* **Restart recovery** – Because receipts are persisted in `SimpleDb`, restarting
  the node or migrating to a new machine preserves outstanding balances and prior
  payouts. Empty receipts are still stored, preventing double-claims after restarts.
* **Auditable history** – The CLI and explorer surfaces can be used to reconcile
  balances following a restart or rollback. Export the history in chunks using the
  `--cursor` flag to avoid replaying the entire dataset at once.

## Governance Controls

The rebate rate is governed alongside other economic parameters:

* **Rate clamps** – `governance::Params` defines the maximum rebate rate a relayer
  can earn per proof. Nodes clamp incoming submissions to the authorised limit
  before they touch the tracker.
* **Policy updates** – Governance proposals can adjust rebate multipliers or
  temporarily disable payouts. Any change applies to subsequent block claims, so
  relayers should watch governance events when planning claim cadence.
* **Operator overrides** – Operators can inspect current parameters through the
  `inflation.params` RPC or governance CLI commands to confirm the rate in effect
  before batching claims.

## Operational Checklist

1. **Instrument dashboards** – Pull `/light_client/top_relayers` and
   `/light_client/rebate_history` into Grafana (or similar) to visualise volume
   per relayer and recent payouts.
2. **Automate audits** – Schedule a job that walks `light_client.rebate_history`
   daily, exporting receipts to cold storage. Use the returned `next` cursor to
   resume incremental scans.
3. **Alert on backlog** – If `pending_total` grows beyond the expected threshold,
   alert miners to mine a block or investigate stalled claims.
4. **Document relayer IDs** – Maintain an internal registry mapping relayer
   identifiers to teams or services so governance can triage issues quickly.
5. **Plan for reorgs** – During network events watch for restored pending
   balances via `rebate_status`; the automatic rollback ensures no CT is lost, but
   relayers should be ready to resubmit proofs if necessary.

Following this runbook keeps rebate incentives transparent, durable, and aligned
with governance policy across the network.
