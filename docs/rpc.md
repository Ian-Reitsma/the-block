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
  signature and proximity, and stores the receipt hash to
  prevent replays. See [docs/localnet.md](localnet.md) for discovery and
  session setup.
- `dns.publish_record` – publishes a signed DNS TXT record to the on-chain
  gateway store.
- `gateway.policy` – fetches the JSON policy document for a domain and
  returns `reads_total` and `last_access_ts` counters.
- `gateway.reads_since?epoch=` – totals reads for the domain since the given
  epoch.
- `analytics` – returns `{reads, bytes}` served for a domain based on finalized
  `ReadAck` batches.
- `microshard.roots.last?n=` – lists the most recent micro‑shard root headers.
- `inflation.params` – returns current subsidy multipliers, industrial backlog
  and utilisation, and rent rate.

  ```bash
  curl -s localhost:26658/inflation.params | jq
  # {"beta_storage_sub_ct":50,"gamma_read_sub_ct":20,
  #  "kappa_cpu_sub_ct":10,"lambda_bytes_out_sub_ct":5,
  #  "industrial_multiplier":100,
  #  "industrial_backlog":0,"industrial_utilization":0,
  #  "rent_rate_ct_per_byte":1}
  ```

- `compute_market.stats` – exposes current compute backlog and utilisation
  metrics.

  ```bash
  curl -s localhost:26658/compute_market.stats | jq
  # {"industrial_backlog":0,"industrial_utilization":0}
  ```

  - `consensus.difficulty` – returns the current proof-of-work difficulty target and timestamp.

    ```bash
    curl -s localhost:26658/consensus.difficulty | jq
    # {"difficulty":12345,"timestamp_millis":1700000000000}
    ```

    The timestamp is in milliseconds; polling once per block (≈1 s) is
    sufficient for monitoring. See [docs/difficulty.md](difficulty.md) for the
    retarget algorithm.

  - `stake.role` – queries bonded CT for a service role.

    ```bash
    curl -s localhost:26658/stake.role?address=$ADDR | jq
    # {"gateway":1000000,"storage":5000000,"exec":0}
    ```
  - `rent.escrow.balance` – returns locked CT per blob or account.
- `settlement.audit` – replays recent receipts and verifies explorer anchors; used in CI to halt mismatched settlements.

## Deprecated / removed endpoints

The 2024 third-token ledger removal eliminated a number of legacy RPC calls.
All methods under the former third-token namespace were removed, and clients
should migrate to the subsidy-centric replacements listed above. Any request
against those paths now returns `-32601` (method not found).
