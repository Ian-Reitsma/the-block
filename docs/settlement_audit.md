# Settlement Audit

The node checkpoints receipts under `state/receipts/pending/<epoch>` before
finalization. The `settlement.audit` RPC streams a summary of each pending
checkpoint so operators can verify signature chains and dispute invalid
entries during the window.

```bash
$ tb-cli audit http://127.0.0.1:8545
epoch 42 receipts 3 invalid 0
```

Use the CLI to inspect pending epochs or integrate the RPC into an explorer
for automated verification. The `tools/indexer` utility ingests
checkpointed receipts into a local database so dashboards can query them and
trigger alerts:

```bash
cargo run -p indexer -- IndexReceipts state/receipts/pending audit.db
```

Set `TB_SETTLE_AUDIT_INTERVAL_MS` to have the node automatically audit pending
epochs on a schedule, writing reports to `state/receipts/audit_latest.json`
and incrementing the `settle_audit_mismatch_total` Prometheus counter when
invalid entries appear. Explorer jobs can poll the indexer for finalized epochs
and alert on mismatches.

Operators can also invoke the `settlement.audit` RPC manually. Sample output:

```text
epoch 42 receipts 3 invalid 1
```

A sample JSON report is available under
`examples/settlement/audit_report.json` for tooling tests and explorer
integration, and the `settlement_rollback` test demonstrates state recovery
when conflicting settlements are detected.
