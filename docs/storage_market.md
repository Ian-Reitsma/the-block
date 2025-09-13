# Storage Market Design

This document outlines the incentive-compatible storage market where nodes
advertise available disk space and accept contracts funded in-chain.

## Offers
Providers publish `StorageOffer` values describing capacity, price per byte,
and retention period.

## Contracts
Clients split files into erasure-coded shards and allocate them across
providers. Each `StorageContract` tracks the provider, shard count, pricing,
and retention window. Contracts are funded from CT balances and only pay for
successful storage.

## Proof of Retrievability
Clients may issue challenges to providers. Failure to respond results in a
slashing event and is tracked via the `retrieval_failure_total` metric.

## RPC Endpoints
- `storage_upload` registers a new contract.
- `storage_challenge` verifies a contract's availability.

## Metrics
- `storage_contract_created_total` counts new contracts.
- `retrieval_failure_total` counts failed challenges.
