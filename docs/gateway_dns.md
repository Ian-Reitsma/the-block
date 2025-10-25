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
  `stake_reference`, and `bid_ct`. Bids must exceed the running minimums and the
  current highest bid.
- `dns.complete_sale` finalises an auction once `end_ts` has elapsed (or
  immediately when `force=true` is supplied for manual settlement/testing).
  Protocol fees are deposited into the treasury hook, royalties are paid to the
  prior owner (if any), and ownership/sale history records are updated before
  the auction is marked `settled`.

### 7.3 Inspecting auctions and history

`dns.auctions` accepts an optional `domain` filter:

```json
{"method":"dns.auctions","params":{"domain":"premium.block"}}
```

The response includes the current auction (if active), persisted ownership
record, and full sale history for auditing.

### 7.4 CLI helpers

`blockctl gateway domain` exposes first-party wrappers for the RPC endpoints:

```bash
blockctl gateway domain list premium.block 2500 --protocol-fee 400 --royalty 150
blockctl gateway domain bid premium.block bidder 3600 --stake staker-ref
blockctl gateway domain complete premium.block --force
blockctl gateway domain status premium.block --pretty
```

All commands reuse the shared JSON helpers and authenticated RPC client. Tests
exercise successful flows, low-bid rejection, auction expiry, and resale royalty
enforcement entirely through the first-party harness.
