# Simulation Framework Manual
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The `sim/` crate models network topologies and economic scenarios. Scenarios are
expressed in YAML and replay deterministically so developers can reproduce bugs
or performance regressions.

## Scenario Schema

A scenario file declares `nodes`, `faults`, and `workloads`:

```yaml
# cluster-basic.yml
nodes:
  - name: validator-a
    region: us-east
  - name: validator-b
    region: eu-west
faults:
  - kind: partition
    targets: [validator-b]
    start_ms: 5000
    duration_ms: 10000
workloads:
  - kind: tx_burst
    rate: 200   # tx/s
    until_ms: 15000
```

`nodes` describe participants and optional metadata. `faults` schedule network
partitions, crashes, or byzantine behaviours. `workloads` generate transactions
or storage reads.

## Network Models

`LatencyModel` and `PartitionModel` plugins plumb into the simulator's event
loop. The default `LatencyModel` reads inter‑region baselines and applies random
jitter; custom models can be injected via `--latency-plugin <lib.so>` for
hardware‑in‑the‑loop testing. `PartitionModel` accepts scripted partitions or
probabilistic drop rates.

## Running a Scenario

From the repository root:

```bash
cargo run --package sim -- --scenario sim/cluster-basic.yml --seed 42
```

The simulator prints per‑node statistics and writes an event log to
`sim/out/cluster-basic-42.log`.

## Deterministic Replay

Each run records a PRNG seed and serializes all events. Re-running with the same
`--seed` and scenario yields identical outcomes. Logs contain a SHA256 hash of
the scenario to detect tampering.

## Further Reading

- Benchmarks: [docs/benchmarks.md](benchmarks.md)
- Gossip chaos harness: [docs/gossip_chaos.md](gossip_chaos.md)
- Compute-market admission scenarios: [sim/compute_market](../sim/compute_market/)
