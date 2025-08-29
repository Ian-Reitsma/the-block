# Network Topologies

The network layer is exercised in `tests/net_gossip.rs` where small clusters
illustrate common peer configurations.

## Three-node mesh

```
A ---- B
 \    /
   C
```

Nodes `A`, `B`, and `C` start fully connected and exchange handshakes.  The
`gossip_converges_to_longest_chain` test mines on each peer and proves that the
mesh converges to the longest chain after temporary forks.

## Partition and rejoin

```
A ---- B    C
```

`partition_rejoins_longest_chain` begins with `A` and `B` connected while `C`
mines in isolation.  When `C` reconnects with a longer fork it is adopted by
the network, demonstrating partition recovery.

## Feature Bits

| Bit | Name | Meaning |
| --- | --- | --- |
| 0x0004 | FEE_ROUTING_V2 | P2P protocol supporting future fee routing |
| 0x0008 | COMPUTE_MARKET_V1 | Compute-market RPCs and workloads |

Peers lacking required bits are rejected during handshake.

## Fuzzed handshake and gossip decoding

The `fuzz/network` harness feeds randomized bytes into
`node::net::message::decode` and the handshake parser.  This ensures malformed
inputs are rejected without panics or resource leaks.  Any crash seeds are saved
under `fuzz/network/artifacts/` with a `repro.sh` helper for triage.

CI runs `cargo fuzz run network` nightly to guard the decoder against
regressions.
