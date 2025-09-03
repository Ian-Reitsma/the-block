# HTTP Gateway – Zero‑Fee Web Hosting

The HTTP gateway is the public entry point for on‑chain web sites. It maps a
`SiteManifestTx` domain to its blob assets, executes optional `FuncTx` WASM
handlers, logs every read via `ReadAck`, and exports analytics without charging
visitors or publishers.

Security considerations are catalogued under
[threat_model/hosting.md](threat_model/hosting.md).

## 1. Request Lifecycle

1. **Accept & Throttle** – `web/gateway.rs` accepts the TCP connection and runs a
   per‑IP token bucket. Exceeding the bucket returns HTTP 429 and logs
   `read_denied_total{reason="rate_limit"}`.
2. **Domain Stake Check** – the `Host` header is verified against the on‑chain
   stake table. Domains without an escrowed deposit receive HTTP 403.
3. **Manifest Resolve** – the published `SiteManifestTx` is fetched by domain
   name. The manifest maps paths to blob IDs and optional WASM function hashes.
4. **Static Blob Stream** – for ordinary paths the gateway pulls erasure‑coded
   shards via `storage/pipeline.rs`, reassembles the blob, and streams bytes to
   the client. No fees are charged and the client decrypts locally if needed.
5. **Dynamic Execution** – `"/api/"` paths invoke the referenced `FuncTx`. The
   WASM bytecode is loaded from the blob store, executed with deterministic fuel
   limits, and its output streamed back to the client.
6. **ReadAck Append** – once the response body is sent, the gateway pushes a
   `ReadAck {manifest_id, path_hash, bytes, client_ip_hash, ts}` into an in‑memory
   queue for later batching.

## 2. Receipt Batching & Analytics

- A background task drains queued `ReadAck`s, writes them to CBOR batches, and
  Merklizes each batch root. Roots anchor on‑chain so auditors can reconstruct
  traffic.
- The `analytics` RPC exposes per‑domain totals computed from finalized batches
  allowing site operators to verify pageviews or ad impressions.

## 3. Subsidy Issuance for Reads

- Finalized read batches mint `READ_SUB_CT` via the block coinbase. The
  formula `γ × bytes` is governed by `inflation.params`.
- Prometheus counter `subsidy_bytes_total{type="read"}` increments with every
  anchored batch so operators can reconcile payouts.

## 4. Abuse Prevention Summary

- **Rate limits** – per‑IP token buckets backed by an Xor8 filter (97 % load, 1.1×10⁻³ FP); governance knob `gateway.req_rate_per_ip`.
- **Stake deposits** – domains bond CT before serving content; slashable on
  abuse.
- **WASM fuel** – deterministic execution with `func.gas_limit_default`.
- **Auditability** – all reads recorded via `ReadAck`; batches with <10 % signed
  acks are discarded and can trigger slashing.

## 5. Operator Visibility

- `gateway.policy` reports current rate‑limit counters and last access time.
- `gateway.reads_since(epoch)` scans finalized batches for historical traffic.
- `analytics` RPC provides aggregated read counts and bytes, suitable for
  dashboarding or advertising audits.

