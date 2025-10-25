# Gateway DNS Publishing and Policy Records
> **Review (2025-09-25):** Synced Gateway DNS Publishing and Policy Records guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Gateways publish domain policies and free-read counters via signed DNS TXT records. The chain does not consult ICANN roots, so only `.block` domains are trusted implicitly; other TLDs must expose the same public key in the public DNS zone before clients honour the on-chain entry. This document explains how records are signed, stored, verified, and queried by clients and auditors.

## 1. Storage Layout

All records are stored in a `SimpleDb` tree chosen by the `TB_DNS_DB_PATH` environment variable (default `dns_db` in the node data directory). Keys:

- `dns_records/<domain>` – UTF-8 TXT payload.
- `dns_reads/<domain>` – little-endian `u64` counting total read queries.
- `dns_last/<domain>` – UNIX seconds of the most recent access.

## 2. Publishing a Record

`dns.publish_record` RPC accepts parameters:

```json
{
  "domain": "example.block",
  "txt": "policy=v1; contact=ops@example.com",
  "pubkey": "<hex ed25519 key>",
  "sig": "<hex signature over domain||txt>"
}
```

Steps:

1. Decode `pubkey` and `sig` as Ed25519 arrays.
2. Concatenate `domain` and `txt`, verify the signature.
3. On success, write `txt` to `dns_records/<domain>`, store `pubkey` under `dns_keys/<domain>`, and initialise `dns_reads`/`dns_last` to `0`.
4. Return `{ "status": "ok" }`.

Invalid signatures yield `ERR_DNS_SIG_INVALID`.

## 3. Retrieving Gateway Policy

`gateway.policy` RPC returns the current TXT record and read statistics when the domain passes verification:

```json
{
  "record": "policy=v1; contact=ops@example.com",
  "reads_total": 42,
  "last_access_ts": 1700000000
}
```

On each query the server:

1. Loads the TXT record.
2. Increments `dns_reads/<domain>`.
3. Updates `dns_last/<domain>` to the current timestamp.
4. Appends a `ReadAck` via `read_receipt::append` so the access can be audited and subsidised.

If the domain is unknown or verification fails, `record` is `null` and counters remain `0`.

`gateway.dns_lookup` exposes the verification status without mutating counters:

```json
{"method":"gateway.dns_lookup","params":{"domain":"example.block"}}
```

The response includes `{ "record": <txt or null>, "verified": <bool> }`.

## 4. Read Counters Since Epoch

`gateway.reads_since` allows auditors to fetch total reads and the last access after a given epoch:

```json
{"method":"gateway.reads_since","params":{"domain":"example.block","epoch":1700000000}}
```

Internally it scans finalised `ReadBatch` files, returning `{ "reads_total": <u64>, "last_access_ts": <u64> }` for transparency.

## 5. Usage Examples

Publish and query via the CLI:

```bash
blockctl rpc "{\"method\":\"dns.publish_record\",\"params\":{...}}"
blockctl rpc "{\"method\":\"gateway.dns_lookup\",\"params\":{\"domain\":\"example.block\"}}"
blockctl rpc "{\"method\":\"gateway.policy\",\"params\":{\"domain\":\"example.block\"}}"
```

## 6. Operational Notes

- Rotate TXT records periodically to advertise new policies or contact points.
- Monitor `dns_reads` and `dns_last` to detect abuse or stale domains.
- The free-read model means clients incur no fees; all read counts feed the `READ_SUB_CT` subsidy via `read_receipt` batching.
- Domains outside `.block` require a public TXT record containing the on-chain `pubkey`. Verification results are cached for one hour and tracked via the `gateway_dns_lookup_total{status}` metric.
- Set `gateway_dns_allow_external = true` in `config.toml` to enable external domains; the default restricts lookups to `.block`.

Keep this guide aligned with `node/src/gateway/dns.rs` whenever the schema or RPC parameters change.

## 7. Premium Domain Auctions & Royalties

Premium `.block` and verified external domains can be listed on-chain via a
first-party auction engine persisted in the same `SimpleDb` namespace:

- `dns_auction/<domain>` – current auction metadata (seller, stake requirement,
  protocol-fee basis points, royalty rate, bids, and status).
- `dns_ownership/<domain>` – active owner, last sale price, and sticky royalty
  configuration carried into resales.
- `dns_sales/<domain>` – historical sales entries (seller, buyer, price,
  protocol-fee cut, royalty cut, timestamp).

### 7.1 Listing a domain

Call `dns.list_for_sale` with JSON parameters:

```json
{
  "domain": "premium.block",
  "min_bid_ct": 2500,
  "stake_requirement_ct": 2500,
  "duration_secs": 86400,
  "protocol_fee_bps": 400,
  "royalty_bps": 150,
  "seller_account": "treasury-account",
  "seller_stake": "stake-ref"
}
```

- `min_bid_ct` must be non-zero; `stake_requirement_ct` defaults to the same
  value and is clamped to the minimum bid.
- `protocol_fee_bps` and `royalty_bps` are clamped to ≤10_000. When relisting a
  previously sold domain, the stored royalty rate is reused and protocol fees
  default to the prior auction’s setting if the request omits a value.
- For resales the `seller_account` must match the stored owner; otherwise
  `AuctionError::OwnershipMismatch` is returned.

Listings return `{ "status": "ok", "auction": { ... } }` with the structured
auction entry including the current bids array.

### 7.2 Bidding and completion

- `dns.place_bid` requires `domain`, `bidder_account`, optional
  `stake_reference`, and `bid_ct`. Stake references are created via
  `dns.register_stake`, which debits the bidder and records an escrow balance
  tied to the provided `reference`. Bids must exceed the running minimums, and
  when a stake requirement is configured the referenced escrow must exist, be
  owned by the bidder, and cover the required amount. Losing bidders are
  unlocked automatically when a higher offer lands.
- `dns.complete_sale` finalises an auction once `end_ts` has elapsed (or
  immediately when `force=true` is supplied for manual settlement/testing).
  Ledger settlement is executed as an atomic batch: the winning bidder is
  debited, seller proceeds and any seller-stake refunds are credited, royalties
  are distributed, and treasury protocol fees are booked. The handler records
  each batch operation’s transaction reference in the sale history so explorers
  can verify the on-ledger transfers.
- `dns.cancel_sale` lets the recorded seller abort an active auction prior to
  settlement. The call releases any locked bidder stake, unlocks the seller’s
  escrow (if present), marks the auction as `cancelled`, and leaves sale
  history untouched so the name can be relisted cleanly.

### 7.3 Inspecting auctions and history

`dns.auctions` accepts an optional `domain` filter:

```json
{"method":"dns.auctions","params":{"domain":"premium.block"}}
```

The response includes the current auction (if active), persisted ownership
record, and full sale history for auditing, including the ledger transaction
references (`ledger_events`) recorded during settlement.

### 7.4 CLI helpers

`blockctl gateway domain` exposes first-party wrappers for the RPC endpoints:

```bash
blockctl gateway domain list premium.block 2500 --protocol-fee 400 --royalty 150
blockctl gateway domain bid premium.block bidder 3600 --stake staker-ref
blockctl gateway domain stake-register stake-ref 2500 --owner bidder
blockctl gateway domain stake-withdraw stake-ref 500 --owner bidder
blockctl gateway domain cancel premium.block --seller treasury-account
blockctl gateway domain complete premium.block --force
blockctl gateway domain stake-status stake-ref --pretty
blockctl gateway domain status premium.block --pretty
```

All commands reuse the shared JSON helpers and authenticated RPC client.
End-to-end tests now exercise winning and losing ledger flows, stake rejection
paths, auction cancellation, stake withdrawals, and resale royalty enforcement
through the first-party integration harness (`node/tests/dns_auction_ledger.rs`).

### 7.5 Stake escrow management

- `dns.register_stake` debits `deposit_ct` from `owner_account`, appends a
  `stake_deposit` ledger event (with `tx_ref`) to the escrow history, and
  persists the updated record under `reference`. Multiple deposits top up the
  same escrow as long as the owner matches.
- `dns.withdraw_stake` credits the owner after reducing the available escrow,
  recording a `stake_withdraw` event for every successful transfer. Locked
  portions (from active bids or ownership requirements) remain intact; the
  handler refuses withdrawals that would dip into the locked balance. Zeroed
  escrows remain in storage so their ledger history stays queryable.
- `dns.stake_status` reports the current escrow (`amount_ct`, `locked_ct`,
  derived `available_ct`, and `ledger_events`) so operators can audit deposits,
  withdrawals, and outstanding locks with concrete transaction references.

Stake entries live alongside auction state in `SimpleDb`. Production flows
should register stake before bidding and can safely withdraw free balance after
auctions settle or cancel. Both `stake-register` and `stake-withdraw` RPC calls
now return the executed `tx_ref` alongside the enriched stake payload so CLI and
explorer tooling can link escrow movements back to on-ledger batches without
relying on off-ledger bookkeeping.
