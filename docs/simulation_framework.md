# Simulation Framework Manual
> **Review (2025-09-25):** Synced Simulation Framework Manual guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

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

## Dependency Fault Harness

Wrapper migrations introduced a dedicated dependency fault harness that lives in
`sim/src/dependency_fault.rs`. The harness spins up a miniature cluster using the
runtime, transport, overlay, storage, coding, crypto, and codec wrappers, then
injects failures to rehearse fallback strategies before rolling out third-party
swaps.

To run the harness:

```bash
cargo run -p tb-sim --bin dependency_fault --features dependency-fault \
  --runtime inhouse --transport quinn --overlay libp2p --storage inhouse \
  --coding reed-solomon --crypto dalek --codec binary \
  --fault transport:timeout --fault coding:panic --duration-secs 10
```

Key CLI flags:

- `--runtime`, `--transport`, `--overlay`, `--storage`, `--coding`, `--crypto`,
  and `--codec` toggle between primary and fallback backends.
- `--fault <target>:<kind>` injects timeouts or panics into the selected wrapper.
- `--duration-secs` and `--iterations` bound each rehearsal run.
- `--output-dir` overrides the default `sim/output/dependency_fault/`
  destination. The harness stores metrics and markdown summaries under
  `sim/output/dependency_fault/<timestamp>_*`.

Each scenario records a machine-readable `metrics.json`, a human-friendly
`summary.md`, and (optionally) an `events.log` capturing injected faults and
recovered operations. Reports enumerate receipts committed by the compute-market
match loop, transport connection failures, overlay claims, coding throughput,
and RPC latency so stakeholders can compare third-party providers with the
fallback stack.

### Adding New Fault Injectors

Fault injectors live under `sim/src/dependency_fault_harness/mod.rs`. To add a new
injector:

1. Extend `FaultTarget` with a descriptive variant and update the CLI help.
2. Teach `FaultInjector::new` and `ScenarioMetrics` to record the new target.
3. Introduce a `run_<target>_probe` helper that exercises the relevant wrapper
   using mock implementations or in-process node utilities.
4. Emit metrics to the scenario report and update tests under
   `sim/tests/dependency_fault.rs` to cover the new path.

### Automation Hooks

Governance and CI can trigger rehearsals via the helper script
`scripts/run_dependency_fault_sim.sh`, which enables the `dependency-fault`
feature, plumbs the desired backends, and propagates policy labels as run
metadata. The script accepts the same CLI flags as the binary so policy changes
can fan out to multiple fallback combinations in nightly jobs.

### Comparing Runs

Use `scripts/compare_dependency_fault.py` to diff two output directories. The
script loads `metrics.json` artifacts, calculates deltas for latency, failure
counts, and codec throughput, and emits a markdown summary suitable for PRs or
governance artefacts.

## WAN Chaos & Attestation Harness

The WAN chaos harness coordinates overlay, storage, and compute faults across
the `Simulation` struct.  The simulator now embeds a `ChaosHarness` that
registers module-specific scenarios (overlay partitions, DHT shard loss, and
compute throttling) and advances them on every `Simulation::step`.  Readiness
metrics derived from the harness surface in snapshots as
`overlay_readiness`, `storage_readiness`, `compute_readiness`, and the aggregate
`chaos_breaches` counter.  CSV dashboards produced by
`Simulation::run` include these columns so operators can correlate economic
outputs with module-specific fault budgets.

Use the new `sim/chaos_lab.rs` binary to continuously exercise the harness,
persist CSV dashboards, and emit signed readiness attestations:

```bash
cargo run -p tb-sim --bin chaos_lab -- \
  --steps 180 \
  --attestations sim/output/chaos/attestations.json \
  --dashboard sim/output/chaos/dashboard.csv
```

`chaos_lab` reads an Ed25519 signing key from `TB_CHAOS_SIGNING_KEY` (hex) or
falls back to an ephemeral key, printing the verifying key to stderr.  The
binary serializes each `ChaosAttestation` via the monitoring crate’s
first-party codec, ensuring both the simulator and the metrics aggregator agree
on payloads.  Operators can feed the generated attestations directly into the
metrics aggregator `/chaos/attest` endpoint (see
[`docs/monitoring.md`](monitoring.md)).

Harness configuration lives in `sim/src/chaos.rs`; extend the registered
scenarios to model additional overlays, storage tiers, or compute pipelines.
Integration coverage in `sim/tests/chaos_harness.rs` exercises breach detection,
recovery, and attestation draft generation. Downstream verification relies on
`metrics-aggregator/tests::chaos_lab_attestations_flow_through_status`, which
posts the emitted artefacts into `/chaos/attest` and asserts `/chaos/status`
alongside the readiness/breach metrics using only first-party crates, and
`chaos_attestation_rejects_invalid_signature`, which tampers with the signature
bytes to ensure forged payloads never reach the readiness cache.

CI and release workflows wire the harness into both `just chaos-suite` and
`cargo xtask chaos`.  The recipes allocate `target/chaos/attestations.json`, run
the `chaos_lab` binary, and surface readiness failures as hard gate blockers
before governance tags advance.

## Deterministic Replay

Each run records a PRNG seed and serializes all events. Re-running with the same
`--seed` and scenario yields identical outcomes. Logs contain a SHA256 hash of
the scenario to detect tampering.

## Identity simulation

The DID simulator (`sim/did.rs`) now assembles documents via
`foundation_serialization::json` builders rather than serde derives so the
binary runs cleanly during full workspace test sweeps without invoking any
third-party stubs.

## Further Reading

- Benchmarks: [docs/benchmarks.md](benchmarks.md)
- Gossip chaos harness: [docs/gossip_chaos.md](gossip_chaos.md)
- Compute-market admission scenarios: [sim/compute_market](../sim/compute_market/)
