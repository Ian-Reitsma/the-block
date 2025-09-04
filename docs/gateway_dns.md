# Gateway DNS Publishing and Policy Records

Gateways publish domain policies and free-read counters via signed DNS TXT records. This document explains how records are signed, stored, and queried by clients and auditors.

## 1. Storage Layout

All records are stored in a `SimpleDb` tree chosen by the `TB_DNS_DB_PATH` environment variable (default `dns_db` in the node data directory). Keys:

- `dns_records/<domain>` – UTF-8 TXT payload.
- `dns_reads/<domain>` – little-endian `u64` counting total read queries.
- `dns_last/<domain>` – UNIX seconds of the most recent access.

## 2. Publishing a Record

`dns.publish_record` RPC accepts parameters:

```json
{
  "domain": "example.com",
  "txt": "policy=v1; contact=ops@example.com",
  "pubkey": "<hex ed25519 key>",
  "sig": "<hex signature over domain||txt>"
}
```

Steps:

1. Decode `pubkey` and `sig` as Ed25519 arrays.
2. Concatenate `domain` and `txt`, verify the signature.
3. On success, write `txt` to `dns_records/<domain>` and initialise `dns_reads`/`dns_last` to `0`.
4. Return `{ "status": "ok" }`.

Invalid signatures yield `ERR_DNS_SIG_INVALID`.

## 3. Retrieving Gateway Policy

`gateway.policy` RPC returns the current TXT record and read statistics:

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

If the domain is unknown, `record` is `null` and counters remain `0`.

## 4. Read Counters Since Epoch

`gateway.reads_since` allows auditors to fetch total reads and the last access after a given epoch:

```json
{"method":"gateway.reads_since","params":{"domain":"example.com","epoch":1700000000}}
```

Internally it scans finalised `ReadBatch` files, returning `{ "reads_total": <u64>, "last_access_ts": <u64> }` for transparency.

## 5. Usage Examples

Publish and query via the CLI:

```bash
blockctl rpc "{\"method\":\"dns.publish_record\",\"params\":{...}}"
blockctl rpc "{\"method\":\"gateway.policy\",\"params\":{\"domain\":\"example.com\"}}"
```

## 6. Operational Notes

- Rotate TXT records periodically to advertise new policies or contact points.
- Monitor `dns_reads` and `dns_last` to detect abuse or stale domains.
- The free-read model means clients incur no fees; all read counts feed the `READ_SUB_CT` subsidy via `read_receipt` batching.

Keep this guide aligned with `node/src/gateway/dns.rs` whenever the schema or RPC parameters change.
