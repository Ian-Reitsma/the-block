# Gateway Read Accounting

Gateway nodes log every served read without charging end users or domain owners.
Reads append `ReadReceipt`s containing `{domain, provider_id, bytes_served, ts}`
under `receipts/read/<epoch>/<seq>.cbor` and batch hourly roots anchored on L1.

Providers are rewarded from a governance-controlled `read_reward_pool` when
receipts finalize. Issuance is capped per region and surfaced via the
`credit_issued_total{source="read"}` metric. Domain credits are never burned.

Abuse is mitigated through per-IP and per-identity token buckets in
`gateway/http.rs`. Exhausted buckets return `429 Too Many Requests` and increment
`read_denied_total{reason="limit"}`.

For dynamic pages, gateways also emit `ExecutionReceipt`s accounting for CPU and
I/O without billing users. Both receipt types are Merklized together for the
hourly anchor.

Use the `gateway.policy` RPC to inspect DNS TXT records and fetch the counters:

```json
{"jsonrpc":"2.0","id":11,"method":"gateway.policy","params":{"domain":"example.com"}}
```

Responses include `reads_total` and `last_access_ts`, enabling owners to audit
traffic while keeping reads free.
