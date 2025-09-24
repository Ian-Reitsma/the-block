# Settlement Audit
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Settlement auditing now spans two complementary feeds:

1. **Consensus checkpoints** – Receipts written under `state/receipts/pending/<epoch>` prior to finalization. These remain documented by the `settlement.audit` RPC and the optional `tools/indexer` pipeline.
2. **Compute-market ledger** – The CT settlement ledger persists an append-only JSON log via `state::append_audit` and exposes the same information over `compute_market.audit`; legacy `*_it` fields remain for compatibility but stay zero in production.

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

Anchors appear with `entity == "__anchor__"` and include the BLAKE3 hash of the submitted receipt bundle. Explorer jobs should persist these markers and render continuity timelines alongside the Merkle roots returned by `compute_market.recent_roots`.

The CLI forwards the same data when compiled with settlement support:

```bash
cargo run -p contract-cli --features full -- compute stats --url http://127.0.0.1:26658
```

The stats command first issues `compute_market.stats`, then fetches provider balances and recent audit entries to present CT exposure per provider (legacy `industrial` columns are retained for tooling compatibility). Use the `--features sqlite-storage` build when only the RocksDB-backed ledger support is required.

## Monitoring Consensus Receipt Audits

Consensus receipts continue to flow through `settlement.audit`. A minimal curl probe looks like:

```bash
curl -s localhost:26658/settlement.audit | jq
```

CI still runs `cargo test -p the_block --test settlement_audit --release` to replay recent checkpoints and fail if explorer indexes diverge from ledger anchors.

Set `TB_SETTLE_AUDIT_INTERVAL_MS` to instruct the node to audit pending epochs periodically. Reports land in `state/receipts/audit_latest.json`, and the Prometheus counter `settle_audit_mismatch_total` increments whenever an invalid entry surfaces.

The `tools/indexer` utility can ingest checkpointed receipts into a SQLite database:

```bash
cargo run -p indexer -- IndexReceipts state/receipts/pending audit.db
```

Explorer jobs should poll for finalized epochs and alert on mismatches between the checkpoint feed and the compute settlement anchors. The `settlement_rollback` integration test demonstrates recovering from divergent settlements by replaying the persisted audit trail.