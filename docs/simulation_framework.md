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

Set `TB_CHAOS_STATUS_ENDPOINT` to fetch a live `/chaos/status` baseline before
the harness runs. `chaos_lab` issues the HTTP request with the in-house
`httpd::BlockingClient`, enforces a 10-second timeout, and decodes the response
manually through `foundation_serialization::json::Value`, mapping fields via
`SnapshotDecodeError` helpers so no serde derives or third-party HTTP stacks are
required. The decoded snapshots are written to
`TB_CHAOS_STATUS_BASELINE` (if provided) and diffed against the freshly emitted
attestations, with the resulting diff persisted to
`TB_CHAOS_STATUS_DIFF` (defaulting to `chaos_status_diff.json`). Setting
`TB_CHAOS_REQUIRE_DIFF=1` forces the run to fail when no changes are detected,
making it safe to gate overlay soaks on fresh regressions.
When no baseline is supplied the harness now writes an explicit empty diff file,
ensuring downstream tooling still captures a deterministically formatted JSON
artefact for auditing and release packaging.

`TB_CHAOS_OVERLAY_READINESS` controls where the per-site readiness rows land.
Each row captures the scenario, module, site, provider, current readiness,
scenario readiness, prior readiness/provider (if a baseline was available), and
window metadata. These rows feed automation (and `cargo xtask chaos`) without
leaving first-party tooling, giving soak harnesses a JSON artefact they can sort
or diff alongside the status snapshot. Set `TB_CHAOS_PROVIDER_FAILOVER` to emit
`chaos_provider_failover.json`; `provider_failover_reports` synthesises
per-provider outages, recomputes readiness, and aborts the run when a simulated
outage fails to drop readiness or register a `/chaos/status` diff entry.

Site-specific readiness can be orchestrated through the
`TB_CHAOS_SITE_TOPOLOGY` environment variable.  Setting

```
TB_CHAOS_SITE_TOPOLOGY="overlay=us-east:0.6:0.1,eu-west:0.4:0.2;compute=us-central:0.5:0.1"
```

overrides the default per-module site roster with named locations, weight
targets, and optional wake-up delays.  The harness automatically normalises
weights, exposes site-level readiness in each attestation draft, and persists
the selected topology so dashboards and the metrics aggregator report identical
module/site/provider breakdowns.  The implementation lives in `ChaosSite::new`
and `ChaosScenario::set_sites`, ensuring every scenario shares a common,
first-party representation that the attestation draft simply clones into its
`site_readiness` vector, including each site’s `ChaosProviderKind` so the
aggregator can emit `chaos_site_readiness{module,scenario,site,provider}`.

Malformed attestation payloads are now rejected explicitly: monitoring-based
tests cover unknown modules, truncated signature arrays, and invalid byte
entries, while the simulator exercises distributed site weighting through
`sim/tests/chaos_harness.rs`.

Harness configuration lives in `sim/src/chaos.rs`; extend the registered
scenarios to model additional overlays, storage tiers, or compute pipelines and
call `configure_sites` to seed per-scenario provider mixes. `Simulation::new`
demonstrates the default overlay mix (`ChaosSite::with_kind("provider-a", …)` and
`ChaosSite::with_kind("provider-b", …)`) so integration tests immediately
exercise the provider-aware site readiness export path.
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
before governance tags advance. `cargo xtask chaos` now fails the gate when
overlay readiness drops, sites disappear, or provider failover drills lack diff
entries, printing per-scenario readiness transitions alongside module totals so
releases stay on first-party guardrails.  `scripts/release_provenance.sh` shells
out to `cargo xtask chaos --out-dir releases/<tag>/chaos` before hashing build
artefacts, and `scripts/verify_release.sh` aborts when the published archive
omits the resulting snapshot/diff/overlay/provider failover JSON files, keeping
release provenance aligned with the automation harness.

Each run also persists a bundle in `chaos/archive/`. `chaos_lab` writes a
run-scoped `manifest.json` containing the file name, byte length, and BLAKE3
digest for every snapshot, diff, overlay readiness table, and provider failover
report, plus a `latest.json` pointer to the newest manifest and a deterministic
`run_id.zip` bundle. Operators can mirror those manifests and the bundle into
long-lived directories or S3-compatible buckets with `--publish-dir`,
`--publish-bucket`, and `--publish-prefix`. Uploads run through the
first-party `foundation_object_store` crate, which wraps the existing HTTP/TLS
client so external SDKs are never required.

## Deterministic Replay

Each run records a PRNG seed and serializes all events. Re-running with the same
`--seed` and scenario yields identical outcomes. Logs contain a SHA256 hash of
the scenario to detect tampering.

## Identity simulation

The DID simulator (`sim/did.rs`) now assembles documents via
`foundation_serialization::json` builders rather than serde derives so the
binary runs cleanly during full workspace test sweeps without invoking any
third-party stubs. Likewise, `sim/src/lib.rs` provides a
`mobile_sync::measure_sync_latency` stub when the `runtime-wrapper` feature is
disabled; the stub logs `mobile_sync_stub_inactive_runtime` and returns a zero
duration so CI and documentation builds retain deterministic output without
linking to external runtime crates.

## Further Reading

- Benchmarks: [docs/benchmarks.md](benchmarks.md)
- Gossip chaos harness: [docs/gossip_chaos.md](gossip_chaos.md)
- Compute-market admission scenarios: [sim/compute_market](../sim/compute_market/)
