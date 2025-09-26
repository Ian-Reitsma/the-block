# Read Receipts and Audit Workflow
> **Review (2025-09-25):** Synced Read Receipts and Audit Workflow guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The gateway records every successful file or API read with a compact
`ReadAck` structure so that subsidy claims and traffic analytics are
verifiable long after a request completes. This document describes the
acknowledgement format, batching semantics, and the tools auditors use to
replay batches and confirm on‑chain totals.

## 1. `ReadAck` structure

`node/src/read_receipt.rs` defines `ReadAck` as a client‑signed tuple:

- `manifest` – 32‑byte identifier of the published manifest or dynamic
  function. This binds the acknowledgement to the content that was served.
- `path_hash` – BLAKE3 hash of the request path. Logging the hash instead of
  the raw URI preserves privacy while still letting auditors reconstruct
  popularity distributions.
- `bytes` – exact byte count returned to the client. Subsidy calculations use
  this value when minting `READ_SUB_CT`.
- `ts` – millisecond timestamp when the read finished. Anchoring a timestamp
  deters replay attacks and allows time‑bounded analytics.
- `client_hash` – salted hash of the client IP address. The salt rotates each
  epoch so observers cannot correlate requests across epochs.
- `pk`/`sig` – the client’s Ed25519 public key and signature over the above
  fields. `ReadAck::verify()` recomputes the message hash and rejects malformed
  signatures or keys.

All fields are serialized with `serde_cbor` so that gateways can append raw
CBOR blobs to on‑disk batches without per‑acknowledgement framing overhead.

## 2. Batching and Merkle roots

Gateways enqueue acknowledgements in memory until an operator‑tunable flush
interval elapses. `ReadBatcher::finalize()` consumes the queue, hashes each
acknowledgement with BLAKE3, and reduces the resulting leaves into a single
Merkle root. The batch header records:

- `root` – 32‑byte Merkle root over the acknowledgements.
- `total_bytes` – sum of `bytes` across the batch.
- `count` – number of acknowledgements included.

The serialized batch is written to
`receipts/read/<epoch>/<sequence>.cbor` and the root is exposed via the
`read_batch_root` field in the block header (`node/src/lib.rs`). When the
containing block finalizes, gateways claim `READ_SUB_CT` proportional to
`total_bytes`.

## 3. Audit flow

Auditors reconstruct traffic with the following steps:

1. Fetch the finalized batch root for a given epoch via the explorer or RPC.
2. Download the corresponding batch file from the gateway or another archival
   node.
3. Recompute hashes for each `ReadAck`, rebuild the Merkle tree, and confirm
   that the root matches the on‑chain value.
4. Sum `bytes` to verify that the minted `READ_SUB_CT` equals
   `γ × total_bytes` for the epoch’s `γ` multiplier.

`tools/analytics_audit` provides a reference implementation of this workflow.

## 4. Related RPCs and metrics

- `gateway.reads_since(epoch)` – returns per‑domain totals derived from
  finalized batches.
- `analytics` – aggregates read counts and bytes for dashboards.
- Prometheus counters `subsidy_bytes_total{type="read"}` and
  `read_denied_total{reason}` reflect subsidy issuance and rate‑limit drops.

## 5. Failure handling

Batches with fewer than 10 % signed acknowledgements are discarded to prevent
unsourced subsidy claims. The gateway always logs unsigned reads with
`allowed=false` so operators can diagnose abusive traffic without skewing
reward totals.

## 6. Examples

```rust
use the_block::ReadBatcher;

let mut batcher = ReadBatcher::new();
// push ReadAck values collected from clients …
let batch = batcher.finalize();
println!("root: {:x?}, bytes: {}", batch.root, batch.total_bytes);
```

For end‑to‑end examples, see `node/tests/subsidy_smoothing.rs` and the audit
utility under `tools/analytics_audit`.
