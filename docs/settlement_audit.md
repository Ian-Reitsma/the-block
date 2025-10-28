# Settlement Audit
> **Review (2025-09-25):** Synced Settlement Audit guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Settlement auditing now spans two complementary feeds:

1. **Consensus checkpoints** – Receipts written under `state/receipts/pending/<epoch>` prior to finalization. These remain documented by the `settlement.audit` RPC and the optional `tools/indexer` pipeline.
2. **Compute-market ledger** – The dual-currency ledger persists an append-only JSON log via `state::append_audit` and exposes the same information over `compute_market.audit`. Each record now carries CT and IT token totals, the oracle snapshot that priced the conversion, and the residual USD that could not be expressed as whole tokens. Explorer jobs, dashboards, and CI artefacts consume both sets of fields directly, so the legacy CT-only ledger path keeps working while automation gains end-to-end visibility into the infrastructure-token side.

Both surfaces must agree. Operators should stream each endpoint, archive the JSON, and compare anchor hashes to detect tampering.

## Inspecting Compute-Market Ledger Events

The durable ledger introduced in `node/src/compute_market/settlement.rs` records every accrual, refund, penalty, and anchor with a monotonic sequence number. Query it through the RPC interface:

```bash
curl -s localhost:26658/compute_market.audit | jq '.[-3:]'
curl -s localhost:26658/compute_market.provider_balances | jq
curl -s "localhost:26658/compute_market.recent_roots?limit=4" | jq '.roots'
```

Each audit record mirrors the `AuditRecord` struct:

```json
{
  "sequence": 57,
  "timestamp": 1695209942,
  "entity": "provider-nyc-01",
  "memo": "accrue_split",
  "delta_ct": 4200,
  "delta_it": 0,
  "balance_ct": 98200,
  "balance_it": 0,
  "anchor": null
}
```

Liquidity records now honour the governance `liquidity_split_ct_ppm` knob. When the split assigns only a fraction of the liquidity share to CT, the ledger records the complementary USD as IT before producing the miner remainder. Auditors should use the oracle snapshot captured alongside the record to verify that `delta_ct` and `delta_it` align with the configured split and the recorded `total_usd_micros`.
Debug assertions in the shared conversion helper now fail fast whenever the minted CT or IT tokens (plus their rounding remainder) drift from the configured USD slices, and a dedicated rounding regression exercises uneven oracle prices to prove that liquidity never exceeds its budget even when remainders spill into miner payouts and `unsettled_usd_micros`.

Anchors appear with `entity == "__anchor__"` and include the BLAKE3 hash of the submitted receipt bundle. Explorer jobs should persist these markers and render continuity timelines alongside the Merkle roots returned by `compute_market.recent_roots`.

The CLI forwards the same data when compiled with settlement support:

```bash
cargo run -p contract-cli --features full -- compute stats --url http://127.0.0.1:26658
```

The stats command first issues `compute_market.stats`, then fetches provider balances and recent audit entries to present CT exposure per provider (legacy `industrial` columns are retained for tooling compatibility). No additional feature flags are required; enable `sqlite-migration` only when importing historical SQLite snapshots.

## Monitoring Consensus Receipt Audits

Consensus receipts continue to flow through `settlement.audit`. A minimal curl probe looks like:

```bash
curl -s localhost:26658/settlement.audit | jq
```

CI still runs `cargo test -p the_block --test settlement_audit --release` to replay recent checkpoints and fail if explorer indexes diverge from ledger anchors.

Set `TB_SETTLE_AUDIT_INTERVAL_MS` to instruct the node to audit pending epochs periodically. Reports land in `state/receipts/audit_latest.json`, and the runtime telemetry counter `settle_audit_mismatch_total` increments whenever an invalid entry surfaces.

The `tools/indexer` utility can ingest checkpointed receipts into a SQLite database:

```bash
cargo run -p indexer -- IndexReceipts state/receipts/pending audit.db
```

Explorer jobs should poll for finalized epochs and alert on mismatches between the checkpoint feed and the compute settlement anchors. The `settlement_rollback` integration test demonstrates recovering from divergent settlements by replaying the persisted audit trail.
