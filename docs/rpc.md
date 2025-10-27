# RPC
> **Review (2025-09-29):** Captured the runtime HTTP client rollout, noted the bespoke server still in place, and refreshed readiness + token hygiene notes.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-29).

## Client configuration

All outbound RPC traffic now routes through the in-house
[`httpd`](../crates/httpd) crate. The async `HttpClient` powers node sidecar
work (telemetry uploads, peer pings, etc.) while the CLI, wallet, probe, and
auxiliary binaries invoke the synchronous `BlockingClient` wrapper that simply
delegates to the shared runtime handle. Both surfaces expose the same request
builder API:

```rust
let client = httpd::HttpClient::default();
let response = client
    .request(httpd::Method::Post, "http://127.0.0.1:26658")?
    .timeout(Duration::from_secs(5))
    .json(&payload)?
    .send()
    .await?;
let envelope: RpcEnvelope<_> = response.json()?;
```

Blocking contexts swap `HttpClient` for `BlockingClient` and remove the `.await`
while retaining every other call. Responses provide `json`, `text`, and
`decode` helpers backed by the canonical codec profiles, and `ClientError`
offers `is_timeout()` for parity with the legacy `reqwest` checks.

> **TLS note:** The client currently supports `http://` endpoints. HTTPS
> integration will land alongside the in-house TLS stack (tracked under the
> runtime roadmap) and the docs will be updated once the transport hooks are in
> place. Remote signer deployments that rely on HTTPS should retain their
> existing TLS termination layer (e.g. stunnel or nginx) until then.

> **URI helpers:** All clients now depend on the new `httpd::uri` primitives for
> parsing and encoding. The helpers currently validate only the `http`, `https`,
> `ws`, and `wss` schemes and intentionally return `Err(UriError::InvalidAuthority)`
> for exotic authority strings. Until the full router ships, integrations should
> stick to the documented schemes to avoid 501 responses from the stub parser.

The CLI and internal tooling continue to use `node/src/rpc/client.rs`, which
reads several environment variables. Operators can tune request behaviour with:

## Server runtime

The node continues to serve JSON-RPC over the bespoke request parser in [`node/src/rpc/mod.rs`](../node/src/rpc/mod.rs). It reads from `runtime::net::TcpListener` and `runtime::io::BufferedTcpStream`, enforces timeouts with `runtime::timeout`, and manually routes method tables while fault-injection hooks fire before payload dispatch. Downstream services—the metrics aggregator, gateway, status surface, explorer, and tooling binaries—now share the first-party [`crates/httpd`](../crates/httpd) router, so tests and examples exercise the same in-house listener. Deleting the ad-hoc parser and replacing it with `httpd::Router` remains a tracked task in `docs/roadmap.md`; once that lands, handlers will inherit codec, telemetry, and keep-alive semantics from `httpd::ServerConfig`. Until then, add new node RPC endpoints by extending the existing match arms and tests under `node/tests/`.

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

### WebSocket surfaces

All RPC WebSocket endpoints now run on the in-house
[`runtime::ws`](../crates/runtime/src/ws/mod.rs) stack. The server side wraps
upgraded sockets in `runtime::ws::ServerStream`, so `/logs/tail`,
`/state/stream`, and `/vm.trace` negotiate RFC 6455 handshakes without relying
on the in-house WebSocket stack (superseding the prior `tokio_tungstenite`
implementation). Frames follow the same semantics across endpoints:

- Text messages carry JSON payloads (`Vec<LogEntry>` for log tails,
  `StateChunk` for state streaming, and execution steps for VM traces).
- Binary frames remain reserved for future compression support. Callers should
  treat them as UTF‑8 JSON for now (the CLI client already attempts a UTF‑8
  decode before deserialising).
- Ping/Pong frames are handled automatically by the server. Applications may
  still send `Ping` to measure liveness; the runtime codec replies with
  `Pong` and surfaces the incoming `Ping` message to callers.
- Close frames trigger a graceful shutdown. `ServerStream::recv` returns
  `Ok(None)` once the handshake completes, and telemetry counters continue to
  increment around the new stream type.

Clients can reuse `runtime::ws::ClientStream` to drive upgrades: generate a
`Sec-WebSocket-Key` with `ws::handshake_key`, issue the GET upgrade, and verify
`ws::read_client_handshake` against the expected accept string. The CLI and
integration tests illustrate this flow end-to-end.

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
- `bridge.relayer_accounting` – returns `(asset, RelayerInfo)` tuples filtered by
  optional `asset`/`relayer` arguments so operators can inspect bonds, rewards,
  penalties, and duty counters without touching sled snapshots.
- `bridge.duty_log` – paginates recorded duty assignments with optional
  `asset`/`relayer` filters and a `limit`, exposing status, reward/penalty, and
  failure reasons for governance and operator audits.
- `bridge.claim_rewards` – redeems a governance-issued `RewardClaimApproval`
  for a relayer. The request accepts `{relayer, amount, approval_key}` and
  returns a `RewardClaimRecord` containing the claim ID, amount, remaining
  allowance, and updated pending balance.
- `bridge.reward_claims` – cursor-paginates the recorded reward claim history.
  Requests accept an optional `relayer`, `cursor`, and `limit` (default 100),
  returning `{claims, next_cursor}` so operators can page through reconciliations
  without materialising the entire retention window.
- `bridge.submit_settlement` – records an external settlement proof for a
  pending withdrawal. The payload contains `{asset, relayer, commitment,
  settlement_chain, proof_hash, settlement_height}` and produces a settlement
  duty entry plus a log record for audit.
- `bridge.settlement_log` – cursor-paginates settlement submissions filtered by
  optional asset, exposing `{settlements, next_cursor}` for dashboards that
  stream relayer proof activity without requesting the whole log.
- `bridge.dispute_audit` – cursor-paginates dispute summaries (pending
  withdrawals, challenges, settlement expectations, and per-relayer outcomes)
  with `{disputes, next_cursor}` so governance tooling can iterate through long
  histories efficiently.
- `bridge.assets` – lists the configured bridge channels (asset identifiers)
  currently persisted on disk.
- `bridge.configure_asset` – declaratively updates channel configuration. All
  numeric fields are optional; omitted values leave the existing configuration
  unchanged, and `clear_settlement_chain` removes any previously configured
  settlement destination.
- `gateway.policy` – fetches the JSON policy document for a domain and
  returns `reads_total` and `last_access_ts` counters.
- `gateway.reads_since?epoch=` – totals reads for the domain since the given
  epoch.
- `gateway.dns_lookup` – returns `{record, verified}` without updating read counters.
- `analytics` – returns `{reads, bytes}` served for a domain based on finalized
  `ReadAck` batches.
- `ad_market.inventory` – returns `{status, distribution, oracle, cohort_prices,
  campaigns}`. `distribution` mirrors the active `DistributionPolicy`
  percentages, `oracle` includes the current `{ct_price_usd_micros,
  it_price_usd_micros}` snapshot, and `campaigns` is an array of
  `{id, advertiser_account, remaining_budget_usd_micros, creatives}` entries
  (creative IDs only) so governance and operators can audit live USD budgets
  without reading sled snapshots.
- `ad_market.distribution` – surfaces the persisted
  `{viewer_percent, host_percent, hardware_percent, verifier_percent,
  liquidity_percent}` split backing subsidy settlements, matching the CLI output
  format for dashboards.
- `ad_market.register_campaign` – accepts a campaign JSON payload (matching
  `ad_market::Campaign`) and registers it with the persistent marketplace,
  returning `{status:"ok"}` on success, `-32000` on duplicates, or `-32603`
  when persistence fails.
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
