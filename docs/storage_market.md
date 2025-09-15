# Storage Market Design

This document outlines the incentive-compatible storage market where nodes
advertise available disk space and accept contracts funded in-chain.

## Offers
Providers publish `StorageOffer` values describing capacity, price per byte,
and retention period.

## Contracts
Clients split files into erasure-coded shards and allocate them across
providers using reputation-weighted Lagrange coding. Each `StorageContract`
tracks the provider, shard count, pricing, retention window, and next payment
block. Contracts are funded from CT/IT balances via the wallet CLI
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

## Explorer
The explorer exposes `/storage/providers` with aggregated provider capacity
and reputation statistics, joining contract data with registered offers.
