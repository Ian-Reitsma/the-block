# Operator Runbook

This runbook captures common SRE procedures for The Block node operators.

## Mempool Spikes
- Monitor `tx_rejected_total{reason="mempool_full"}` on `/metrics`.
- Increase `TB_MEMPOOL_MAX` or scale out additional nodes.
- Use `purge_loop` to prune expired transactions.

## State Corruption
- Stop the node immediately.
- Restore the latest snapshot and apply `.diff` files in order.
- Validate the resulting state root using `account_proof`.

## Block Halt
- Check connectivity and peer bans with `peer_error_total`.
- Verify clocks and rebuild from snapshot if consensus stalled.
- File an incident report and circulate the block hash once resolved.
