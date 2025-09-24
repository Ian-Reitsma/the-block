# Sharding Model
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

This document sketches the proposed sharding design for The‑Block and serves as a living specification.

## Overview

The chain is split into **micro‑ledgers**.  Each shard maintains an independent state root and block height.  A global "macro block" periodically checkpoints the tip of each shard and aggregates the coinbase subsidies.

```
macro block k
 ├─ shard 0 height h0 root r0
 ├─ shard 1 height h1 root r1
 └─ ...
```

## Cross‑Shard Transactions

Transactions carry a source and destination shard identifier.  A `CrossShardEnvelope` directs the transaction to the source shard for execution and forwards any outbound messages to the destination shard through the inter‑shard queue.

## Macro Blocks

Macro blocks are produced less frequently than shard blocks.  A macro block lists the latest accepted block for each shard, includes the cumulative subsidies, and commits to the inter‑shard message queue.  Nodes use this checkpoint to prune shard histories and reconcile rewards.

## Inter‑Shard Queue

Each shard maintains an outbound queue of messages destined for other shards.  Macro blocks commit to the Merkle root of all queued messages since the previous macro block, providing replay protection.

## Diagram

```
shard A ---- message ----\
                           > macro block M_k
shard B ---- message ----/
```

Macro block `M_k` references the heads of shards `A` and `B` and the Merkle root of their exchanged messages.

## Addressing

Account identifiers embed a 16-bit shard ID as a hexadecimal prefix:

```
0001:abcd...  # account `abcd...` on shard 1
```

The `ledger.shard_of` RPC method decodes this prefix so clients can map any
address to its shard without replicating the address logic.
