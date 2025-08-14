# Network Topologies

## Three-node mesh

```
    A
   / \
  B---C
```

All peers connect to each other. This layout underpins the `mesh_converges` test.

## Partition and rejoin

```
A   B
 \ /
  C

# partition -- C isolated

A   B   C

# rejoin

A---B
 \ /
  C
```

Node `C` partitions from `A` and `B`, mines an alternate chain, then rejoins. The
`partition_rejoin_longer_fork` test exercises this scenario.
