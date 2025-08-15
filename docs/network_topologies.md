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
