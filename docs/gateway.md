# Gateway Read Accounting

Gateway nodes log every served read without charging end users or domain owners.
This section captures the full free-read pipeline from request handling to
reward issuance.

## 1. ReadReceipt Lifecycle

1. **Request arrival** – `gateway/http.rs` accepts an HTTP GET, validates
   headers, and resolves the domain through `gateway/dns.rs`.
2. **Chunk streaming** – encrypted chunks are fetched from storage peers via
   `storage/client.rs` and streamed to the requester; the gateway never sees
   plaintext because decryption happens client-side.
3. **Receipt creation** – after the response completes, the gateway calls
   `read_receipt::append` with `{domain, provider_id, bytes_served, ts,
   dynamic=false}`. Receipts are stored as CBOR files under
   `receipts/read/<epoch>/<seq>.cbor`.
4. **Batching** – an hourly job `read_receipt::batch` Merklizes the pending
   receipts, writes the root to `receipts/read/<epoch>.root`, and queues an L1
   anchor via `settlement::submit_anchor`.
5. **Finalization** – once the on-chain anchor confirms, a settlement watcher
   moves all files into `receipts/read/<epoch>.final` and invokes
   `issue_read` to mint provider credits.
6. **Dynamic pages** – if server-side code runs, `gateway/exec.rs` also emits an
   `ExecutionReceipt` (CPU-seconds, disk IO). Its hash is batched alongside the
   `ReadReceipt` root so compute and storage receipts anchor together.

## 2. Credit Issuance for Reads

- `credits/ledger.rs::read_reward_pool` holds the reward balance seeded via the
  `read_pool_seed` governance parameter.
- `credits/issuance.rs::issue_read` validates finalized receipts, enforces
  per-region caps, mints credits, and increments
  `credit_issued_total{source="read",region}`.
- Decay and expiry apply exactly like other sources; balances live per
  provider.

## 3. Abuse Prevention

- `gateway/http.rs` enforces per-IP and per-identity token buckets configured by
  `tokens_per_minute` and `burst_tokens`.
- Bucket exhaustion returns HTTP `429 Too Many Requests` and increments
  `read_denied_total{reason}`.
- Denied reads still append a `ReadReceipt` with `allowed=false` so misuse is
  auditable without billing users.

## 4. Visibility & Analytics

- `gateway.policy` RPC exposes `{reads_total, last_access_ts}` counters for
  domain owners.
- `gateway.reads_since(epoch)` scans finalized batches to aggregate reads per
  domain.
- Operators can reconstruct traffic analytics without ever seeing credit
  deductions or user fees.

