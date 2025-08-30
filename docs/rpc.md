# RPC

## Compute-market error codes

| Code   | Meaning           |
|--------|-------------------|
| -33000 | no price data     |
| -33001 | invalid workload  |
| -33002 | job not found     |
| -33099 | internal error    |

## Endpoints

- `mempool.stats?lane=` – returns `{size, age_p50, age_p95, fee_p50, fee_p90}`
  for the requested lane.
- `localnet.submit_receipt` – accepts a hex‑encoded assist receipt, verifies
  signature and proximity, accrues credits, and stores the receipt hash to
  prevent replays.
- `dns.publish_record` – publishes a signed DNS TXT record to the on-chain
  gateway store.
- `gateway.policy` – fetches the JSON policy document for a domain.
- `microshard.roots.last?n=` – lists the most recent micro‑shard root headers.
