# Storage Market Design
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

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

## Explorer
The explorer exposes `/storage/providers` with aggregated provider capacity
and reputation statistics, joining contract data with registered offers.