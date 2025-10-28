# Read Receipts and Audit Workflow
> **Review (2025-10-25, evening):** Read acknowledgements now attach zero-knowledge readiness proofs and identity commitments via the first-party `zkp` crate. Gateway/node operators can tune enforcement with `--ack-privacy` or the `node.{get,set}_ack_privacy` RPCs, and ledger submission emits `read_ack_processed_total{result="invalid_privacy"}` when proofs fail under `observe` mode.
> **Review (2025-10-26, evening):** The `read_ack_privacy` integration suite now
> reuses a `concurrency::Lazy` fixture so the readiness snapshot and signature
> material are generated once per test run, cutting redundant RNG/proof setup
> without introducing third-party cells.
> **Review (2025-09-25):** Synced Read Receipts and Audit Workflow guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The gateway records every successful file or API read with a compact
`ReadAck` structure so that subsidy claims and traffic analytics are
verifiable long after a request completes. This document describes the
acknowledgement format, batching semantics, and the tools auditors use to
replay batches and confirm on‑chain totals.

## 1. `ReadAck` structure

`node/src/read_receipt.rs` defines `ReadAck` as a client‑signed tuple with
explicit hosting and campaign metadata:

- `manifest` – 32‑byte identifier of the published manifest or dynamic
  function. This binds the acknowledgement to the content that was served.
- `path_hash` – BLAKE3 hash of the request path. Logging the hash instead of
  the raw URI preserves privacy while still letting auditors reconstruct
  popularity distributions.
- `bytes` – exact byte count returned to the client. Subsidy calculations use
  this value when minting `READ_SUB_CT`.
- `ts` – millisecond timestamp when the read finished. Anchoring a timestamp
  deters replay attacks and allows time‑bounded analytics.
- `client_hash` – BLAKE3 hash of the serving domain concatenated with the
  client IP octets (`hash(domain || client_ip)`). This locks the acknowledgement
  to the observed caller while keeping the raw IP off-chain.
- `pk`/`sig` – the client’s Ed25519 public key and signature over the
  acknowledgement preimage. `ReadAck::verify()` recomputes the message hash and
  rejects malformed signatures or keys.
- `domain` – canonical domain that served the request.
- `provider` – storage/hosting identifier inferred for the response.
- `campaign_id`/`creative_id` – optional identifiers emitted by the advertising
  marketplace when an impression reserves budget for a campaign.
- `selection_receipt` – optional proof of the on-device auction outcome. When
  present, the node replays the receipt to ensure the clearing price matches
  `max(runner_up_quality, resource_floor)` and that the attestation is
  well-formed (SNARK preferred, TEE accepted while circuit proofs stabilize).
- `readiness` – optional `AdReadinessSnapshot` captured when the gateway attaches
  campaign metadata. The snapshot commits to rolling viewer/host/provider
  counters and is accompanied by a zero-knowledge proof.
- `zk_proof` – optional `ReadAckPrivacyProof` binding the acknowledgement to the
  readiness commitment while hiding the viewer identity salt derived from the
  signature.

All fields serialize through the first-party binary codec. The Merkle hash for
each acknowledgement incorporates every field above (including optional
campaign metadata) so downstream audits observe the exact domain/provider pair
that earned the read and can link impressions to campaign settlements. Legacy
gateways may still ingest CBOR archives for replay, but new batches emit the
first-party `.bin` format exclusively.

`ReadAck::reservation_discriminator()` derives a per-ack 32-byte key from the
manifest, path hash, timestamp, client hash, and signature so concurrent fetches
of the same asset never collide in the advertising marketplace.

### Selection receipt attestation

Wallets attach a `SelectionReceipt` whenever an impression spends campaign
budget. Nodes verify the transcript by recomputing the composite resource floor,
runner-up quality, and clearing price before accepting the acknowledgement.
Attestations are preferred as SNARK proofs (non-empty circuit identifiers plus
proof bytes) but TEE reports remain an accepted fallback while circuits roll
out; missing or malformed attestations are counted explicitly so operators can
chart wallet compliance.

## 1.1 Privacy commitments

The first-party `zkp` crate exposes two helper proofs that keep operators out of
the identity path while still demonstrating service quality:

- `ReadinessPrivacyProof` commits to the rolling readiness snapshot (window
  length, thresholds, viewer/host/provider counts, readiness flag, and
  timestamp) using a deterministic blinding derived from a node-local seed.
- `ReadAckPrivacyProof` hashes the acknowledgement payload together with the
  readiness commitment and an identity commitment derived from the client hash
  and a salt hashed from the Ed25519 signature.

Nodes running in `enforce` mode reject any acknowledgement whose proofs fail.
`observe` mode accepts the read but emits
`read_ack_processed_total{result="invalid_privacy"}` so dashboards capture the
anomaly; `disabled` mode skips verification entirely for controlled migrations.

### HTTP signing contract

Gateways no longer synthesize placeholder keys or signatures when logging a
read. Instead, the caller must supply the acknowledgement pre-image alongside
the request so the gateway can verify the signature before enqueueing the
`ReadAck`. Clients attach the following headers to every static asset request:

- `X-TheBlock-Ack-Manifest` – hex-encoded 32-byte manifest identifier.
- `X-TheBlock-Ack-Pk` – hex-encoded Ed25519 public key.
- `X-TheBlock-Ack-Sig` – hex-encoded 64-byte Ed25519 signature over the
  `ReadAck` payload (`manifest || path_hash || bytes || ts || client_hash`).
- `X-TheBlock-Ack-Bytes` – decimal byte count the client expects to receive.
- `X-TheBlock-Ack-Ts` – millisecond timestamp chosen by the client when the
  read completes.

The gateway recomputes `path_hash` from the request path, derives
`client_hash = blake3(domain || client_ip_octets)`, and rejects the request when
the signature fails to verify or the declared byte count differs from the
materialized response. Legacy fixtures that still rely on zeroed signatures are
only supported by compiling the node with the `legacy-read-acks` feature.

## 2. Gateway ingestion and node worker

Gateways enqueue acknowledgements in memory until an operator‑tunable flush
interval elapses. `ReadBatcher::finalize()` consumes the queue, hashes each
acknowledgement with BLAKE3 (including domain, provider, and campaign fields),
and reduces the resulting leaves into a single Merkle root. The batch header
records the root, byte total, and acknowledgement count before writing the
payload to `receipts/read/<epoch>/<sequence>.bin`.

The gateway forwards every signed acknowledgement to the node over an
`mpsc` channel. `spawn_read_ack_worker()` in `node/src/bin/node.rs` drains this
channel, attaches the current `AdReadinessSnapshot`, and calls
`Blockchain::submit_read_ack` which validates signatures and (depending on the
configured privacy mode) the readiness and acknowledgement proofs. The worker
increments `read_ack_processed_total{result="ok|invalid_signature|invalid_privacy"}`
so operators can surface rejected signatures or proofs in telemetry. Successful
acknowledgements populate epoch-scoped byte ledgers keyed by viewer, host,
hardware provider, verifier, and liquidity pool account addresses; ad
impressions simultaneously reserve campaign budget using the
reservation discriminator described above.

### Privacy verification modes

- **CLI:** `node run --ack-privacy={enforce|observe|disabled}` sets the runtime
  enforcement level before the worker starts. Enforce rejects invalid proofs,
  observe logs and counts them, and disabled skips verification.
- **RPC:** `node.get_ack_privacy` reports the active mode, and
  `node.set_ack_privacy {"mode": "observe"}` switches modes at runtime while
  persisting the updated configuration.

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
The worker clears per-epoch accumulators as soon as a block finalizes, so
auditors sampling the live ledger should query before the next epoch boundary
if they wish to compare raw acknowledgement bytes with the pending payout map.

## 4. Related RPCs and metrics

- `gateway.reads_since(epoch)` – returns per‑domain totals derived from
  finalized batches.
- `analytics` – aggregates read counts and bytes for dashboards.
- Runtime telemetry counters `subsidy_bytes_total{type="read"}`,
  `read_denied_total{reason}`, `read_ack_processed_total{result}`,
  `read_selection_proof_verified_total{attestation}`,
  `read_selection_proof_invalid_total{attestation}`, and the
  `read_selection_proof_latency_seconds{attestation}` histogram reflect subsidy
  issuance, rate‑limit drops, acknowledgement validation outcomes, attestation
  mix, and selection-proof verification latency. Sustained growth in
  `read_ack_processed_total{result="invalid_signature"}`,
  `{result="invalid_privacy"}`, or repeated `read_selection_proof_invalid_total`
  spikes should trigger the governance playbook described in
  [governance.md](governance.md#read-acknowledgement-anomaly-response) and the monitoring response loop that correlates the spike with offending domains.

## 5. Subsidy distribution and advertising settlement

`Blockchain::finalize_block` no longer routes `READ_SUB_CT` exclusively to the
miner. Instead the epoch byte ledger fuels a governance‑controlled split across
viewer wallets, hosting domains, hardware vendors, verifiers, and the liquidity
pool. The resulting per‑role CT totals are persisted in the block header via the
`read_sub_*_ct` fields alongside matching advertising fields
(`ad_*_ct`) sourced from settled campaign impressions. Pending ad settlements
are flushed at the same time so explorers and settlement tooling can reconcile
impressions to campaign payouts.

## 6. Failure handling

Batches with fewer than 10 % signed acknowledgements are discarded to prevent
unsourced subsidy claims. The gateway always logs unsigned reads with
`allowed=false` so operators can diagnose abusive traffic without skewing
reward totals.

## 7. Examples

```rust
use the_block::ReadBatcher;

let mut batcher = ReadBatcher::new();
// push ReadAck values collected from clients …
let batch = batcher.finalize();
println!("root: {:x?}, bytes: {}", batch.root, batch.total_bytes);
```

For end‑to‑end examples, see `node/tests/subsidy_smoothing.rs` and the audit
utility under `tools/analytics_audit`.
