# RPC

## Client configuration

The CLI and internal tooling use `node/src/rpc/client.rs`, which reads several
environment variables. Operators can tune request behaviour with:

- `TB_RPC_TIMEOUT_MS` – base timeout in milliseconds (default `5000`).
- `TB_RPC_TIMEOUT_JITTER_MS` – extra random jitter added to the timeout
  (default `1000`).
- `TB_RPC_MAX_RETRIES` – number of retries after transport errors (default `3`).
  The exponential backoff multiplier caps at `2^30` once the retry attempt
  reaches 31 (`MAX_BACKOFF_EXPONENT` in
  [`node/src/rpc/client.rs`](../node/src/rpc/client.rs)), so attempts beyond 30
  reuse that multiplier while still adding jitter to each request.
- `TB_RPC_FAULT_RATE` – probability for fault injection during chaos testing.
  Values outside the inclusive `[0.0, 1.0]` range are clamped, and `NaN`
  entries are ignored to guarantee a well-defined probability.

The test-only `EnvGuard` helper restores any pre-existing environment values on
drop so overrides never leak across cases.

Regression coverage exercises both retry saturation and the sanitized fault
probability. Run

```bash
cargo test -p the_block --lib rpc_client_backoff_handles_large_retries -- --nocapture
cargo test -p the_block --lib rpc_client_fault_rate_clamping -- --nocapture
```

to confirm the exponential multiplier caps at the documented `2^30` ceiling and
that clamped `TB_RPC_FAULT_RATE` values never panic `gen_bool` during chaos
testing.

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

- `compute_market.provider_balances` – returns CT balances for every provider persisted in the settlement ledger. Providers are sorted lexicographically (matching the Merkle root computation) and the payload mirrors `BalanceSnapshot` from `node/src/compute_market/settlement.rs` with `provider`, `ct`, and a legacy `industrial` field that remains zero in production.

  ```bash
  curl -s localhost:26658/compute_market.provider_balances | jq
  # {"providers":[{"provider":"alice","ct":4200,"industrial":0}]}
  ```

- `compute_market.audit` – streams the most recent settlement events, including
  accruals, refunds, penalties, and anchor markers. Each object matches the
  `AuditRecord` struct with `sequence`, `timestamp`, CT deltas (plus a legacy `delta_it` field), the updated
  running balances, and (for anchors) the `anchor` hex string recorded in
  `metadata.last_anchor_hex`.

  ```bash
  curl -s localhost:26658/compute_market.audit | jq '.[-2:]'
  # [
  #   {"sequence":19,"entity":"provider-nyc-01","memo":"accrue_split",...},
  #   {"sequence":20,"entity":"__anchor__","memo":"anchor","anchor":"…"}
  # ]
  ```

- `compute_market.recent_roots?limit=` – lists the latest Merkle roots for the
  settlement ledger (default 32) as hex strings produced by the same Blake3
  fold used in `compute_root`. Use these roots to prove continuity between audit
  records and explorer snapshots.

  ```bash
  curl -s "localhost:26658/compute_market.recent_roots?limit=4" | jq '.roots'
  # ["3c5d…", "97ab…", "42f1…", "1be9…"]
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
- `settlement.audit` – replays consensus settlement receipts and verifies explorer anchors; CI invokes this endpoint to halt mismatched settlements. Pair it with `compute_market.audit` to confirm the CT ledger emits matching anchors (legacy industrial fields remain for compatibility).
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

The 2024 reimbursement-ledger retirement eliminated a number of legacy RPC calls.
All methods under the former reimbursement namespace were removed, and clients
should migrate to the subsidy-centric replacements listed above. Any request
against those paths now returns `-32601` (method not found).

Endpoints returning fees expose CT accounting (selectors remain for tests). Fee reports such as
`mempool.stats` and settlement receipts include `pct_ct` or separate `fee_ct`
and `fee_it` fields to track splits between consumer and industrial lanes.
