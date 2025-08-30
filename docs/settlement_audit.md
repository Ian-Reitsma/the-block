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
for automated verification.
