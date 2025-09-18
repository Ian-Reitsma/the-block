# RPC

## Compute-market error codes

| Code   | Meaning           |
|--------|-------------------|
| -33000 | no price data     |
| -33001 | invalid workload  |
| -33002 | job not found     |
| -33099 | internal error    |

## Endpoints

- `mempool.stats?lane=` – returns `{size, age_p50, age_p95, fee_p50, fee_p90, fee_floor}`
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
- `gateway.dns_lookup` – returns `{record, verified}` without updating read counters.
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

- `compute_market.stats` – exposes current compute backlog, utilisation,
  cumulative processed units, and spot price metrics. Weighted and raw median
  prices remain in the payload for operators who rely on the historic bands,
  and the pending queue snapshot is returned for CLI introspection.

  ```bash
  curl -s localhost:26658/compute_market.stats | jq
  # {"industrial_backlog":0,"industrial_utilization":0,"industrial_units_total":0,
  #  "industrial_price_per_unit":0,"industrial_price_weighted":null,
  #  "industrial_price_base":null,"pending":[]}
  ```

- `compute.job_cancel` – cancels an active job and rolls back resources.

  - Parameters: `job_id` (string), optional `reason` (`client`|`provider`|`preempted`).
  - Returns: `{ok: true}` on success or `{error: "unknown_job"|"already_completed"|"unauthorized"}`.
  - Side effects: releases the scheduler slot, refunds any locked fees, and adjusts reputation.
  - Telemetry: increments `scheduler_cancel_total{reason}`.
  - Example:

    ```bash
    curl -s -d '{"method":"compute.job_cancel","params":{"job_id":"abc123"}}' \
      -H 'Content-Type: application/json' localhost:26658
    ```

  - Requires standard RPC auth headers.
  - See [docs/compute_market.md#cancellations](compute_market.md#cancellations) for semantics and race-condition notes.

  - `consensus.difficulty` – returns the current proof-of-work difficulty target, retune hint, and timestamp.

    ```bash
    curl -s localhost:26658/consensus.difficulty | jq
    # {"difficulty":12345,"retune_hint":2,"timestamp_millis":1700000000000}
    ```

    The timestamp is in milliseconds; polling once per block (≈1 s) is
    sufficient for monitoring. See [docs/difficulty.md](difficulty.md) for the
    retarget algorithm.

  - `vm.trace?code=` – WebSocket endpoint streaming execution traces for the
    provided hex-encoded WASM or bytecode. Requires the node to run with
    `--enable-vm-debug` and is intended for development only.

  - `stake.role` – queries bonded CT for a service role.

    ```bash
    curl -s localhost:26658/stake.role?address=$ADDR | jq
    # {"gateway":1000000,"storage":5000000,"exec":0}
    ```
  - `rent.escrow.balance` – returns locked CT per blob or account.
- `settlement.audit` – replays recent receipts and verifies explorer anchors; used in CI to halt mismatched settlements.
- `dex.escrow_status?id=` – prints `{from,to,locked,released}` for a pending
  escrow.

  ```bash
  curl -s localhost:26658/dex.escrow_status?id=7 | jq
  # {"from":"alice","to":"bob","locked":100,"released":20}
  ```

- `dex.escrow_release?id=&amount=` – releases a partial payment and updates the
  escrow root.

  ```bash
  curl -s localhost:26658/dex.escrow_release?id=7\&amount=40 | jq
  # {"released":60,"root":"ab34…"}
  ```

- `dex.escrow_proof?id=&index=` – retrieves a Merkle proof for a prior
  release.

  ```bash
  curl -s localhost:26658/dex.escrow_proof?id=7\&index=1 | jq
  # {"amount":40,"proof":["aa..","bb.."]}
  ```

## Deprecated / removed endpoints

The 2024 third-token ledger removal eliminated a number of legacy RPC calls.
All methods under the former third-token namespace were removed, and clients
should migrate to the subsidy-centric replacements listed above. Any request
against those paths now returns `-32601` (method not found).

Endpoints returning fees expose mixed CT/IT accounting. Fee reports such as
`mempool.stats` and settlement receipts include `pct_ct` or separate `fee_ct`
and `fee_it` fields to track splits between consumer and industrial lanes.
