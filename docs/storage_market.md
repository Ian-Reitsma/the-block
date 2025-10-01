# Storage Market Design
> **Review (2025-09-30):** Captured in-house crypto/erasure/fountain defaults and refreshed rollout guidance.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This document outlines the incentive-compatible storage market where nodes
advertise available disk space and accept contracts funded in-chain.

## Offers
Providers publish `StorageOffer` values describing capacity, price per byte,
and retention period.

## Contracts
Clients split files into erasure-coded shards and allocate them across
providers using reputation-weighted Lagrange coding. Each `StorageContract`
tracks the provider, shard count, pricing, retention window, and next payment
block. Contracts are funded from CT balances via the wallet CLI (legacy industrial columns remain zeroed for compatibility)
(`blockctl storage upload`), reserving `price_per_block * retention` upfront and
only paying for successful storage.

## Proof of Retrievability
Clients may issue random chunk challenges to providers. Successful proofs
increment `retrieval_success_total`; failures result in slashing, removal from
allocation sets, and bump `retrieval_failure_total`.

## RPC Endpoints
- `storage_upload` registers a new contract.
- `storage_challenge` verifies a contract's availability.

## Metrics
- `storage_contract_created_total` counts new contracts.
- `retrieval_success_total` counts successful challenges.
- `retrieval_failure_total` counts failed challenges.
- `storage_provider_rtt_ms{provider}` records RTT histograms for individual providers.
- `storage_provider_loss_rate{provider}` records recent loss rate observations per provider.

Provider profiling persists in `provider_profiles/<id>`, capturing EWMA throughput, RTT,
loss, success rate, and the adaptive chunk ladder position chosen for each provider. The
node derives logical bandwidth quotas from `Settlement::balance_split(provider)`; the
current implementation treats each credit as 1 MiB of allocatable capacity. When a
provider exhausts its quota or is marked for maintenance, the placement routine skips it
and emits maintenance state via telemetry and RPC. CLI operators can inspect profiles with
`blockctl storage providers` (optionally `--json`) and toggle availability using
`blockctl storage maintenance <provider_id> --maintenance <true|false>`. The JSON-RPC
interface exposes the same data and controls via `storage_provider_profiles` and
`storage_provider_set_maintenance`.

These counters are emitted directly from the RPC helpers in `node/src/rpc/storage.rs` whenever uploads or challenges complete, and they are only available when the `telemetry` feature flag is enabled. Operators relying on contract or challenge telemetry should ensure telemetry is compiled in and that Prometheus scrapes the node endpoint.

## Coding abstraction and configuration

All encryption, erasure, fountain, and compression primitives flow through the shared
`coding` crate. The node loads a [`coding::Config`](../crates/coding/src/config.rs) from
`config/storage.toml`, which exposes the default chunk ladder, Reed–Solomon parity counts,
fountain settings, and compressor level. Each stored manifest records the active
algorithms so data can be decoded even after the configuration changes. Operators can
adjust algorithms or tuning parameters by editing `config/storage.toml` and reloading the
node configuration; runtime reloads also refresh the coding registry so subsequent uploads
use the new settings.

- Set `erasure.algorithm = "xor"` to exercise the in-house XOR fallback coder. The
  pipeline still emits Reed–Solomon metadata in manifests, but the coder records its
  canonical algorithm (`erasure_alg = "xor"`) so repairs and retrieval know to expect the
  reduced parity guarantees. The repair worker automatically downgrades expectations when
  multiple data shards are missing and logs the `algorithm_limited` skip reason instead of
  repeatedly failing reconstruction.
- Adjust `fountain.symbol_size` or `fountain.rate` to tune the in-house LT coder. The
  default `algorithm = "lt-inhouse"` seeds packets deterministically so `storage::repair::fountain_repair_roundtrip` and the node tests can peel losses without extra metadata.
- Set `compression.algorithm = "rle"` to switch to the lightweight run-length compressor
  when the default hybrid `lz77-rle` backend must be avoided. Telemetry reports
  `algorithm="rle"` so dashboards can compare compression ratios across rollout cohorts.
- Because the configuration lives alongside other storage settings, operators can stage
  partial rollouts by first updating a canary node's `storage.toml`, validating telemetry,
  then promoting the change to the wider fleet once confidence is established.

Telemetry surfaces the health of these primitives through
`storage_coding_operations_total{stage,algorithm,result}` counters and the
`storage_compression_ratio{algorithm}` histogram. These metrics make it easy to correlate
success and failure rates across algorithms while tracking real compression effectiveness
in production.

## Explorer
The explorer exposes `/storage/providers` with aggregated provider capacity
and reputation statistics, joining contract data with registered offers.
