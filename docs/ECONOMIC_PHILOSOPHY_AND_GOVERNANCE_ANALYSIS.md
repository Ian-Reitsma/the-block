# Economic Philosophy and Governance Analysis

This handbook collates the monetary, telemetry, and governance reasoning that underpins The Block. Each Part links code, docs, and operator touchpoints. Treat every entry here as part of the spec: if the runtime disagrees, fix the docs first, not the code.

## Part XII · BlockTorch Compute Framework Strategy

### 1. Vision & 1% Developer Mindset
- **Determinism-first**: BlockTorch outputs must reproduce identical receipts whether the job runs on `blocktorch/` Metal kernels, the upcoming CUDA port, or any future SDK plug-in. That determinism fuels pricing, proof budgets, and ledger integrity.
- **Economics-first telemetry**: Every BlockTorch metric (allocation balance, proof latency, slashing depth) traces back to a receipt and a governor gate. If a metric is missing from this doc, log it, wire it through `/wrappers`, and update this page before merging.
- **Operator-ready cadence**: Telemetry, dashboards, CLI/governor status, runbooks, and spec links are the deliverables. Once you finish a code story, ask: can the on-call operator see the proof latency, kernel hash, and governor intent trace without opening a PR? If not, keep iterating.

### 2. Key Outcomes (linking to the plan)
| Outcome | Spec tie-points | Doc references |
| --- | --- | --- |
| Hardware-neutral proof loop | Kernel hashes, ORCHARD logging, receipt metadata | [`docs/blocktorch_compute_integration_plan.md`](docs/blocktorch_compute_integration_plan.md) · this Part XII |
| Economic alignment | SLA slashing → settlement → explorer + CLI evidence | `docs/overview.md#document-map`, `docs/architecture.md#compute-marketplace`, `docs/operations.md#telemetry-wiring` |
| Operator telemetry | Aggregator `/wrappers`, Grafana panels, CLI/governor status | `docs/operations.md#telemetry-wiring`, this Part XII |

### 3. Artifact Schema
- **BlockTorch Receipt Metadata**: Each receipt must embed `kernel_variant.digest` (SHA256 over the vector of Metal/CUDA shader sources), `profiler_log_epoch` (timestamped pointer to `/tmp/orchard_tensor_profile.log` + hashed entries), `benchmark_commit` (the `benchmarks.json` path, e.g., `/tmp/bench/abc123/benchmarks.json`), and `proof_latency_ms` per lane.
- **Determinism Lab Log**: Bundle `blocktorch` commit hash, kernel digest, driver version, `benchmarks.json`, `ORCHARD_TENSOR_PROFILE` path, `tensor_profile_reset` state, and GPU accelerator metadata. Store hashed copy in `docs/reproducibility/labs/<job-id>.json` (create the directory as needed) and cite the file in PRs.
- **Allocator epoch**: Parse `/tmp/orchard_tensor_profile.log` to confirm `alloc`/`free` counts match per job; export `orchard_alloc_free_delta{job_id}` in `node/src/telemetry` and promote it through `metrics-aggregator` and Grafana panels.

### 4. Telemetry + Runbook Wiring
1. **Node telemetry** (`node/src/telemetry.rs`, `node/src/telemetry/receipts.rs`): encode the receipt metadata, `proof_verification_latency`, `sla_breach_count`, `receipt_drain_depth`, `orchard_alloc_free_delta`, and the latest `benchmark_commit`. Guard instrumentation with the `telemetry` feature and reuse `telemetry::metric!` macros where possible.
2. **Metrics aggregator** (`metrics-aggregator/telemetry.yaml`, `/wrappers` exporters): ingest the new metrics, add them to the `/wrappers` manifest, and update `monitoring/tests/snapshots/wrappers.json`. Document the new hash in the PR description.
3. **Grafana monitoring** (`monitoring/`): refresh the dashboards with panels for `snark_prover_latency_seconds{lane}`, `proof_verification_latency`, `orchard_alloc_free_delta`, `sla_breach_depth`, `kernel_variant.digest`, and `benchmark_commit`. Re-run `npm ci --prefix monitoring && make monitor` and commit the JSON + `/wrappers` snapshot.
4. **CLI/governor outputs**: `cli/src/compute.rs`, `cli/src/governor.rs`, and `tb-cli governor status` must mention proof latency buckets, benchmark commit, kernel digest, the `blocktorch_aggregator_trace` hash, and aggregated telemetry state. Add the new fields under `BlockTorch job timeline` output and document `TB_BLOCKTORCH_KERNEL_VARIANT_DIGEST` / `TB_BLOCKTORCH_BENCHMARK_COMMIT` so operators can seed the timeline when the instrumentation cannot capture the artifacts automatically.
5. **Explorer ingestion**: `explorer/src/routes/receipts` should parse the new metadata fields and persist them so `/compute/sla/history` includes kernel digests and benchmark links.
6. **Runbook entries**: Document these steps and the `oracle`/`governance` interplay inside `docs/operations.md#telemetry-wiring` (see the updated section). Include failure reproduction steps referencing `blocktorch/scripts/bootstrap_codex.sh` and `benchmarks/run.py`.

### 5. Governance & Completion Signals
- Link every new governor knob to `TB_BLOCKTORCH_*` env vars (see `node/src/config.rs`), update `governance/src/params.rs`, and describe the shadow/apply transitions in `docs/operations.md#telemetry-wiring`. Capture the governor intent story via `tb-cli governor status --rpc <endpoint>` (include metric hashes for cross-check). Document rollback instructions, telemetry pivot points, and aggregator trace IDs for slashing events in the plan and this doc.

### 6. Next Immediate Actions (start wiring)
1. **Node telemetry**: add receipt metadata exporters, proof latency histograms, and kernel hash labels to `node/src/telemetry.rs`. Validate them via unit tests and ensure they obey the `telemetry` feature gate.
2. **Aggregator wiring**: add the new metrics in `metrics-aggregator/telemetry.yaml`, run `/wrappers` snapshot scripts, and update `monitoring/tests/snapshots/wrappers.json` & watchers.
3. **Monitoring refresh**: rebuild Grafana dashboards, mention kernel hash/benchmark panels, and capture the new JSON + screenshot references for the PR.
4. **CLI/Governor**: extend `cli/src/compute.rs` and `cli/src/governor.rs` to emit BlockTorch job metadata (kernel hash, benchmark commit, proof latency). Document runtime flags (e.g., `--blocktorch-status`) and add acceptance tests verifying consistent formatting.
5. **Explorer**: ensure `/compute/sla/history` returns the enriched metadata and the CLI `contract-cli explorer sync-proofs` entry logs the kernel digest + benchmark commit.
6. **Operations runbook**: update `docs/operations.md#telemetry-wiring` with tasks for wiring metrics and aggregator exposures, include new attacker-run steps referencing `blocktorch_compute_integration_plan` and `TB_BLOCKTORCH_*` env instructions.
7. **Feedback loop**: after wiring, schedule a telemetry verification drill (run `scripts/fuzz_coverage.sh` + `cargo test -p the_block --test replay` + `just test-full` + `npm ci --prefix monitoring && make monitor`) and archive logs + `/wrappers` hashes per `AGENTS.md` instructions before tagging the PR.

### 7. References & Links
- [`docs/blocktorch_compute_integration_plan.md`](docs/blocktorch_compute_integration_plan.md) for step-by-step instructions, checklists, and completion criteria. Keep this plan in sync with Part XII.
- [`docs/architecture.md#compute-marketplace`](docs/architecture.md#compute-marketplace) for system overview; the new material in this file expands on the integration story.
- [`docs/operations.md#telemetry-wiring`](docs/operations.md#telemetry-wiring) for runbook updates.
- `node/src/telemetry.rs`, `metrics-aggregator/telemetry.yaml`, `monitoring/`, `cli/src`, `governance/src`, `explorer/src`, and `blocktorch/` (AGENTS + README) are the key code stubs mentioned throughout this spec.
