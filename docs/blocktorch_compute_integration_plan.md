## BlockTorch + Compute Marketplace Integration Plan

### 1. Vision & Guiding Principles
- **Spec-first**: Every change must start in documentation. This document is the canonical plan—if implementation diverges, the docs need updating first. That includes the BlockTorch subrepo, the compute marketplace, and any future SDK plug-ins.
- **Hardware-neutral proof loop**: Jobs should run deterministically on `blocktorch/` (already shipping) and on the upcoming CUDA backend, while remaining extensible enough for third-party adopters of our SDK plugin architecture (think beyond Apple/NVIDIA to future custom tensor fabrics). The same receipt/proof pipeline must execute across these targets without divergence in ledger state, computation pricing, or telemetry.
- **Economic alignment**: SLA slashing, receipts, explorer signals, and governance knobs must narrate the same story. Proof verification budgets (goal `<100ms`), receipt anchoring, and settlement must be audited through replay + settlement checks before promotion.
- **Operations-ready**: Add telemetry, dashboards, governor gates, and audit docs in lockstep so operators can monitor, trace, and rollback if needed.

This plan is explicitly connected to [`docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md#part-xii`](docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md#part-xii-blocktorch--the-compute-framework-strategy). That Part XII text restates the vision, links to this plan, and outlines the telemetry, governance, and CLI signals that must appear before code merges. Reference it when you report progress, cite completion criteria, or file PRs so reviewers follow the spec chain from this plan through the architecture and operations docs.

### 2. Scope & Assumptions
- **Codeowners**: `blocktorch/` already implements the BlockTorch metal runtime. The CUDA backend is tracked in the BlockTorch repo (external). The compute marketplace (`node/src/compute`, `governance`, `metrics-aggregator`, `monitoring`) orchestrates the job lifecycle.
- **Dependencies**:
  - BlockTorch repo: kernel definitions, gradient serialization, proof metadata.
  - `node/src/settlement`, `node/src/receipts`, and `node/src/telemetry` for end-to-end flow.
  - Explorer & dashboards must show receipts + slashing events (`metrics-aggregator`, `monitoring`, `docs/overview.md`, `docs/operations.md`).
  - Governance knobs live in `governance/src/params.rs` and `node/src/config.rs`.
- **Targets**: `blocktorch/` (already deterministic), CUDA (new custom backend), and the future SDK plugin layer (pluggable compute fabrics with our APIs).

### 2.a Metal-Backend Orientation
- **Read the canonical policies**: start with `blocktorch/AGENTS.md`, follow the workflow checklist, then mirror any changes into the local README sections that summarize “Contributor Protocol”. Every directory that adds a README must mention this file.
- **Repository cartography**:
  - `blocktorch/metal-tensor/` houses the core: `metal/` for production sources (`common/`, `core/`, `kernels/`, `runtime/`), `tests/` for contiguity/autograd coverage, `docs/` for design notes, and `AGENTS.md` that governs tensor-level edits.
  - `experimental/` keeps the PyTorch bridge and FlashAttention kernels under `orchard_ops/`, `benchmarks/`, `tests/`, `kernel_lib/`, `data/`, and `runs/` (all but docs ignored by Git). Build this only when `-DORCHARD_BUILD_EXPERIMENTAL=ON`.
  - `scripts/`, `docs/`, and `benchmarks/run.py` encode build/profile automation; `bootstrap_codex.sh` and `cmake/` contain helper toolchains.
  - `third_party/googletest` is trimmed; configure `-DFETCHCONTENT_FULLY_DISCONNECTED=ON` when isolating builds.
- **Feature cues to mirror in the plan**:
  - Rank-eight shape metadata, gradient logging, zero-copy transfers, and allocation profiling (`ORCHARD_TENSOR_PROFILE`) are all recorded inside `metal-tensor/metal/common/Profiling.h` and surfaced to the compute marketplace through receipts/metadata. Ensure the plan mentions how to propagate these diagnostics into `node/src/telemetry`.
  - Autograd instrumentation (requires_grad, Tensor::grad, Node/Edge graph, detach semantics) underpins proof determinism; annotate the metadata plan with `Tensor::detach`, `Tensor::is_alias_of`, and `tensor_profile_reset`.
  - Metal/CPU kernel parity (elementwise add, multiply, matmul, reductions, mean, transpose, division) ensures deterministic behavior. Include steps to catalog kernel versions (hashes) inside job receipts.
  - Benchmark harness (`python benchmarks/run.py`) writes `/tmp/bench/<commit>/benchmarks.json` and `ORCHARD_TENSOR_PROFILE` log references. Link these artifacts to the DoD (proof verification <100ms) via telemetry/traces.
  - Build/test commands: `cmake -S . -B build -G Ninja`, `cmake --build build`, `ctest --output-on-failure`, `python benchmarks/run.py`, `tensor_profile_reset`, `tensor_profile_clear_log`. Document in plan where each command must run and what logs to collect.
  - Radiate the same environment expectations to `node`/`governance`: ensure `TB_*` env knobs align with `ORCHARD_*` toggles so telemetry gating remains consistent.

### 2.b Metal Tensor Internals (Advanced Dive)
- **Tensor core**: `metal-tensor/metal/core/tensor/Tensor.h` defines the eight-dimensional API (empty, view, slice, add/mul/div, matmul, sum, mean, transpose, detachment, gradient hooks). Use this file when re-evaluating API stability or the ABI contract for receipts.
- **Autograd nodes**: `metal-tensor/metal/core/autograd/*Backward.cpp` (Add, Mul, Div, Matmul, Mean, Sum, Transpose, View) and `Node.cpp` describe how gradients are constructed; include these files when documenting determinism or ProofBudget instrumentation.
- **Runtime & allocation**:
  - `metal-tensor/metal/runtime/Allocator.h` logs every allocation/free pair to `/tmp/orchard_tensor_profile.log` so aggregator dashboards can validate paired counts and detect leaks; mention these logs in the telemetry plan.
  - `MetalContext.*` and `runtime/runtime_cpu.cpp` manage device/command queue pooling; when guiding CUDA/SDK plugin ports, replicate the queue semantics.
- **Tests**: `metal-tensor/tests/tensor_tests.cpp`, referenced in Section 4.6, covers zero copy transfers, view/slice mutability, clone/detach semantics, alignment, host/device round-trips, profiling logs, and autograd gradients; align compute-market fixtures with these exact scenarios so determinism remains stable.
- **Experimental path**: `blocktorch/experimental/` houses the PyTorch bridge (`orchard_ops`, `scripts`), FlashAttention kernels (`kernel_lib`), and `benchmarks/run.py` artifacts; note that enabling this path requires `-DORCHARD_BUILD_EXPERIMENTAL=ON` and `USE_FLASH_ATTN=2`, and its outputs end up in `experimental/runs/` plus `runs/` for diagnostics; highlight in plan how compute-market jobs ingest these artifacts.
- **Docs / telemetry UI surfaces**:
  - `blocktorch/docs/project_status.md`, `docs/tensor.md`, and the README series are the narrative anchors; every spec change mentioned in this plan must ripple through them with identical wording/metrics and mention `AGENTS.md` updates.
  - `blocktorch/openapi.json` enumerates HTTP/TCP endpoints for profiling downloads and telemetry ingestion; when you add a new metric, update this schema and cite the path in the plan (e.g., `GET /blocktorch/receipts`).
  - `blocktorch/web/public` holds the static UIs that surface benchmark metadata + kernel hashes; keep the manifest in parity with `monitoring` dashboards and mention the build steps in the completion matrix so operators know how to rebuild the portal for new metrics or receipts.
- **Scripts & automation**:
  - `blocktorch/scripts/bootstrap_codex.sh` and the `cmake/` helpers orchestrate environment bootstrapping; reuse these commands in deterministic labs and document them in the plan as the canonical reproduction steps for new hardware (include exact flags).
  - `blocktorch/benchmarks/` directory (Python harness + runner) powers the proof verification timing data; always attach the generated `/tmp/bench/<commit>/benchmarks.json` plus the `experimental/runs/<commit>` metadata, and note `USE_FLASH_ATTN` toggles when referencing FlashAttention baselines.

### 3. Desired Outcomes
| Outcome | Metrics |
| --- | --- |
| Submit job → produce receipt/proof → verify <100ms → settle → slash on breach | Deterministic replay, settlement audit, `compute_market.match_loop_latency_seconds`, `snark_prover_latency_seconds` |
| Visibility | Explorer receipts + slashing timeline, telemetry dashboards, `/wrappers` docs, CLI status commands |
| BlockTorch spec | Published Part XII bundle in `docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md` with determinism requirements, artifact formats, proof verification budget, and hardware compatibility |
| SDK plugin readiness | Clear interface in `blocktorch/` + `node/src/compute/matcher.rs` for external fabrics, documented in new plan doc |

### 4. Detailed Workstreams (Developer Instructions + Checklists)

#### 4.1 Spec & Communication
- **Goal**: Capture BlockTorch + compute market expectations before touching code; keep the plan tied to the blocktorch policies.
- **Actions**:
  1. Update `docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md#part-xii` with the new spec bundle (determinism, artifact formats, proof budget, supported hardware) and cite the guidance from `blocktorch/README.md` plus `blocktorch/AGENTS.md`.
  2. Link the spec to `docs/architecture.md#compute-marketplace` and `#ledger-and-consensus` so the broader architecture reflects the new proof loop, and mention this plan alongside those sections.
  3. Notify subsystem owners from `docs/overview.md#document-map` before rolling code—track owner approvals and mention `AGENTS.md` references in the PR description.
  4. Maintain a deterministic lab log that includes:
     - Metal build commit hash + `python benchmarks/run.py` output path (`/tmp/bench/<commit>/benchmarks.json`),
     - `ORCHARD_TENSOR_PROFILE` log path and `tensor_profile_reset` usage,
     - Kernel fingerprint metadata (include `metal-tensor/metal/kernels` file list + hash) that will travel in receipts.
- **Checklist**:
  - [ ] Doc bundle describes SUBMIT → PROOF → VERIFY -> SETTLE → SLASH flow for each hardware target, referencing kernel metadata + telemetry.
  - [ ] Include determinism lab steps (input hash, kernel version, proof metadata, benchmark artifact location) that engineers must reproduce on Metal + CUDA + SDK plugin harness, with pointers to blocktorch docs.
  - [ ] Confirm this plan, `blocktorch/AGENTS.md`, and owner routing instructions are referenced in the doc update and every related README.

#### 4.2 BlockTorch Repo Integration
- **Goal**: Align external BlockTorch repo outputs with `blocktorch/` expectations and compute marketplace inputs.
- **Actions**:
  1. Define canonical artifact schema: job metadata, gradient serialization, SNARK proof descriptors, GPU kernel fingerprints (pull from `metal-tensor/metal/kernels`), profiler dumps, and CUDA build IDs.
  2. Establish sync points: versioned releases in BlockTorch repo, exported `ORCHARD_TENSOR_PROFILE` logs, and ledger proofs that mention job IDs; mention the `benchmarks/` output structure so artifacts are discoverable (`/tmp/bench/<commit>/benchmarks.json`, `runs/`).
  3. Add automation (scripts, CI) that pulls or vendors BlockTorch artifacts into `blocktorch/` / `node` for deterministic execution; reuse `blocktorch/scripts/bootstrap_codex.sh` and `cmake/` helpers to configure `metal-tensor` builds from the compute repo.
  4. Document how `third_party/googletest` bridging works when building on Linux vs macOS to keep cross-platform deterministic.
  5. Record the expected provenance of each artifact: commit hash, kernel file list, `ORCHARD_TENSOR_PROFILE` log checksum, GPU driver version, and cross-check in `docs/blocktorch_compute_integration_plan.md`.
- **Checklist**:
  - [ ] Artifact schema documented, versioned, and mirrored in `docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md`, referencing `blocktorch/docs/` + `metal-tensor/docs/design_spec_tensor_v0.md`.
  - [ ] CI hook verifying BlockTorch release signature before ingestion; include steps from `.github/workflows/macos.yml` for reproducing locally.
  - [ ] Script (captures commands from `blocktorch/scripts`) for replicating GPU kernel builds and bundling them in the compute repo, plus placeholder for CUDA build steps.
  - [ ] Document how `experimental/` FlashAttention kernels fit in (enable with `-DORCHARD_BUILD_EXPERIMENTAL=ON` and `USE_FLASH_ATTN=2`).

#### 4.3 Hardware Execution Stack
- **Targets**: Metal (existing), CUDA (planned), SDK plugin future proof.
- **Actions**:
  1. BlockTorch backend: capture telemetry hooks, ensure `ORCHARD_TENSOR_PROFILE` instrumentation is stable (log at `/tmp/orchard_tensor_profile.log`), snapshot `metal-tensor/metal/kernels` versions + GPU driver metadata, and tie job reproducibility steps into `node/src/telemetry` via `receipt_metadata`.
     - Propagate allocator logs from `metal-tensor/metal/runtime/Allocator.h` (alloc/free pairs, device label) into `node/src/telemetry`. The plan must describe how aggregator panels parse the `/tmp/orchard_tensor_profile.log` lines and correlate them with receipt IDs.
     - Kernel provenance now hashes the Metal kernel bundle (`metal-tensor/metal/kernels`), the CMake cache, the root CMake configuration, and the runtime version stamp file (`metal-tensor/runtime_version.txt`) so receipts carry the true runtime variant digest.
  2. CUDA backend: design API (compute kernels, verification budgets, deterministic scheduler, proof export) in the BlockTorch repo; produce matching metadata (kernel file list, driver version, commit hash) and push via `ReceiptStore`.
     - Mirror MetalContext/queue semantics from `metal-tensor/metal/runtime/MetalContext.*` when designing CUDA contexts so command queue pooling + profiling behave similarly.
  3. SDK plugin: design plugin trait for new fabrics (abstract compute device, deterministic scheduler, proof emitter, cost/performance hints, `probe`s) inside `node/src/compute/mod.rs`; document plugin lifecycle (init, run, proof emit, cleanup) referencing `blocktorch/metal-tensor/runtime/` for device management.
  4. Surface instructions for operators: highlight `tensor_profile_reset`, `tensor_profile_clear_log`, and `benchmarks/run.py` usage; ensure each hardware target documents how to trigger the harness and record `ORCHARD_TENSOR_PROFILE`.
  5. Include a sync checklist for `blocktorch/docs/` (tensor md) to confirm that any API change pushes updates there and into `docs/subsystem_atlas.md`.
- **Checklist** for each target:
  - [ ] Job execution reproducibility steps recorded (input hashes, kernel digest, runtime environment, `benchmarks/run.py` output path).
  - [ ] Proof emission stabilized by `node/src/compute/receipt.rs` and verified in `node/src/telemetry/receipts.rs`; receipts include `metal-tensor` metadata (kernel list, profiling logs).
  - [ ] Verification budget documented (<100ms) and telemetry measured (`snark_prover_latency_seconds`, `compute_market.match_loop_latency_seconds{lane}`); mention where telemetry hooks live (`blocktorch/metal/common/Profiling.h`, `node/src/telemetry`).
  - [ ] SDK trait documented with sample plugin and plugged into the compute courier so new fabrics can be gated with governor knobs.

#### 4.4 Economic & Ledger Integration
- **Goal**: Tie SLA slashing and receipts into the broader economic story with BlockTorch metadata.
- **Actions**:
  1. Wire receipts/SLAs to explorer and governance (update `node/src/settlement.rs`, `governance/src/params.rs`, `docs/economics_and_governance.md`) and mention the BlockTorch artifacts (kernel hashes, profiler logs) on the settlement entries.
  2. Ensure SLA breaches trigger slashing metrics in `node/src/telemetry.rs`, record proof budget overruns, and journal entries in `node/src/settlement.rs` with `BlockTorchJobReceipt`.
  3. Make sure receipts anchor to blocks (`node/src/ledger` path), expose CLI endpoints (`cli/src/compute.rs`, `cli/src/governor.rs`), and show up in explorers (extend `explorer/src/routes/receipts` schema).
- **Checklist**:
  - [ ] Replay + settlement audit coverage for touched ledger flows (`cargo test -p the_block --test replay`, `settlement_audit` release test); include logs for kernel hash verification and `ORCHARD_TENSOR_PROFILE`.
  - [ ] Governance knobs documented and wired to `TB_*` env vars if needed (`node/src/config.rs`, `governance/src/params.rs`).
  - [ ] Explorer + CLI status endpoints mention BlockTorch receipts/slashes (`cli/src/compute.rs`, `cli/src/explorer.rs`, `explorer/src/routes/receipts`) and surface meta-requests for `benchmark` artifact links.
  - [ ] `docs/economics_and_governance.md` outlines how proof verification latency (<100ms) ties into issuance and slashing, referencing the telemetry metrics from 4.5.

#### 4.5 Observability & Dashboards
- **Goal**: Make the proof loop visible end-to-end and connect `ORCHARD_TENSOR_PROFILE` to Prometheus.
- **Actions**:
  1. Add metrics in `node/src/telemetry` for BlockTorch job lifecycle (`proof_verification_latency`, `sla_breach_count`, `receipt_drain_depth`, `orchard_alloc_free_delta`).
     - Parse `/tmp/orchard_tensor_profile.log` after each compute job so the allocator alloc/free delta and log hash can travel through `tensor_profile_epoch` plus the `orchard_alloc_free_delta` gauge.
  2. Wire new metrics through `metrics-aggregator` and refresh Grafana dashboards in `monitoring`; ensure the JSON includes `ORCHARD_TENSOR_PROFILE` log paths and kernel hashes as labels.
  3. Document telemetry expectations in `docs/operations.md#telemetry-wiring` (list new metrics + panel IDs) and update aggregator `/wrappers` docs with their export names.
  4. Extend `blocktorch/metal/common/Profiling.h` to expose labels consumed by the aggregator or duplicate them through `node/src/telemetry` hooks.
  5. Add Grafana panels capturing:
     - Proof verification latency per lane (`snark_prover_latency_seconds{lane}`),
     - SLA slashing counts + depth,
     - Allocation profiling (alloc/free parity) tied to `ORCHARD_TENSOR_PROFILE`.
  6. Document panel IDs and `/wrappers` exposure in `docs/operations.md` and `docs/overview.md` for quick operator reference.
  7. Track `experimental/benchmarks/run.py` output and `experimental/runs/` artifacts so the same hardware baseline (Metal + FlashAttention builds) is reported alongside proof latencies; use the `benchmarks.json` to validate kernel hashes and throughput claims.
- **Checklist**:
  - [ ] `npm ci --prefix monitoring && make monitor` re-run to refresh dashboards and Grafana JSON, capturing the new BlockTorch panels.
  - [ ] Document new metrics and panel references in `docs/operations.md`.
  - [ ] Telemetry gating features remain guarded by the `telemetry` cargo feature.
  - [ ] `/wrappers` docs mention the new metrics + kernel hash metadata, and the aggregator config lists them explicitly.

#### 4.6 Testing Matrix & Verification
- **Goal**: Make sure the compute+BlockTorch pipeline is deterministic, auditable, and performant.
- **Actions**:
  1. Add deterministic test fixtures covering Metal + CUDA + plugin trait harness (possibly in `tests/chaos.rs` style).
  2. Ensure `just lint`, `just fmt`, `just test-fast` run locally; for ledger touches also run `just test-full`, `cargo test -p the_block --test replay`, `cargo test -p the_block --test settlement_audit --release`, and `scripts/fuzz_coverage.sh`.
  3. Document the DoD in this plan: “Submit job → produce receipt/proof → verify <100ms → settle → slash on breach → visible in explorer + dashboards”.
  4. Include `blocktorch/metal-tensor/tests/tensor_tests.cpp` results as part of the determinism narrative; tie the test logs back to the compute job fixtures.
  5. Run `cmake --build build --target check` and `ctest --output-on-failure` (blocktorch) and attach their logs to PRs as evidence of determinism.
  6. Capture benchmark artifacts from `python benchmarks/run.py` (Metal) and the CUDA harness; attach `/tmp/bench/<commit>/benchmarks.json` and `experimental/runs/<commit>` metadata to the PR and mention them in the DoD so operators see the same driver/platform info that BlockTorch uses.
  7. Validate `ORCHARD_TENSOR_PROFILE` produces paired alloc/free counts and that the aggregator consumes those counts before closing the proof loop.
- **Checklist**:
  - [ ] Deterministic fixture for each hardware target with logs attached.
  - [ ] Replay + settlement audit logs appended to PR alongside Metal/CUDA test outputs.
  - [ ] Benchmark verifying `<100ms` on Metal/CUDA/proposed plugin, with JSON linked in the PR.
  - [ ] `ORCHARD_TENSOR_PROFILE` paired alloc/free counts documented in the test output.

#### 4.7 Release & Ops Readiness
- **Goal**: Provide operators (via governor, CLI, dashboards) the ability to control the new loop.
- **Actions**:
  1. Gate new governors in `node/src/launch_governor` with shadow/apply modes described in `docs/operations.md`, include the BlockTorch job proof timeline, and mention the `ORCHARD_TENSOR_PROFILE` log that proves determinism.
  2. Expose CLI commands/status (`tb-cli governor status`, `governor intents`, compute status) referencing proof loop and new telemetry; CLI output must mention the latest benchmark commit, kernel hash set, and proof verification latency.
  3. Document incident runbook additions in `docs/operations.md#troubleshooting-playbook`, covering `ORCHARD_TENSOR_PROFILE` toggles, `blocktorch/scripts/bootstrap_codex.sh`, and step-by-step reproduction of failing proof verification.
  4. Provide fallback instructions for CUDA vs Metal vs SDK plugin via `docs/operations.md`, including which `TB_BLOCKTORCH_*` env knobs to flip and how they impact aggregator metrics.
- **Checklist**:
  - [ ] Shadow governor gates for slashing/receipt enabling exist with fallback instructions.
  - [ ] CLI status commands mention BlockTorch job/proof timeline, latest benchmark commit, and kernel set.
  - [ ] Runbook entry references this plan and includes reproduction steps + telemetry pivots (including `ORCHARD_TENSOR_PROFILE`).
  - [ ] Operators can toggle CUDA/Metal/plugin targets via documented `TB_BLOCKTORCH_*` env vars and measure the impact through `metrics-aggregator`.

### 5. Definition of Done (DoD)
1. **Documentation**: This plan plus referenced doc bundle published; new `docs/ECONOMIC...` section & telemetry entries committed.
2. **Integration**: BlockTorch artifacts feed `blocktorch/`, CUDA backend instrumentation built (or stubbed with documented TODO mirroring §15), and SDK plugin trait documented with sample harness.
3. **Economics**: Receipts anchor to blocks, slashing metrics emitted, explorer + CLI surfaces updated, governance knobs in place.
4. **Observability**: Metrics aggregated, Grafana dashboards refreshed, `/wrappers` docs updated, `monitoring/` JSON checked in.
5. **Testing**: `just lint`, `just fmt`, `just test-fast`, `just test-full`, replay test, settlement audit, fuzz coverage reports attached; proof verification budget verified (<100ms) on targeted hardware.
6. **Operator Playbooks**: `docs/operations.md` updated with telemetry wiring, troubleshoot steps, and references to this plan; governor status commands mention the new loop.

### 5.a Excellence & “1% Developer” Criteria
- **Sustain the 1% dev mindset** — every touch must exceed obvious expectations. Treat spec updates, docs, telemetry, dashboard assets, and engineer-facing output as the living contract. When you write a log line, imagine the incident response team reading it during a high-stakes outage.
- **Authoritative documentation** — plan references, code comments, and dashboards must be literate, precise, and include line/column references when pointing at source files (`metal-tensor/metal/core/tensor/Tensor.h:32`). Use the document style from `blocktorch/docs/project_status.md` (chronological, declarative) when summarizing progress.
- **Completion signals** (excellence pillars):
  1. All hardware paths (Metal, CUDA, future SDK plugin) have a reproducibility note (kernel hash, driver, `benchmarks.json`, `ORCHARD_TENSOR_PROFILE` epoch) committed before code merges.
  2. Every new receipt field references a doc section + aggregator panel in the PR description; unresolved questions are filed as TODOs mirrored in `AGENTS.md §15`.
  3. Telemetry dashboards mention the same metric labels as the underlying `node/src/telemetry` exports + `/wrappers`. No orphaned panels.
  4. CLI/governor output references the latest benchmark commit, the kernel hash set, and the proof verification latency buckets for at least the last five blocks.
  5. Gov gates can run in shadow mode with toggled instrumentation, and the plan documents rollback steps, telemetry pivot points, and aggregator trace IDs for slashing events.

### 5.b Completion Matrix by Subsystem
| Subsystem | Key Artifacts | Completion Indicator | Owner Review Needed |
| --- | --- | --- | --- |
| `blocktorch/metal-tensor` | `metal/core/tensor/Tensor.*`, `metal/runtime/Allocator.h`, `metal/common/Profiling.h`, `tests/tensor_tests.cpp` | Documentation for API invariants, kernel hash export, allocator log pipeline, autograd nodes, and test outcomes stored in `blocktorch/docs/` and referenced in this plan. Build + test logs (`cmake --build build --target check`, `ctest`) attached to PR. | BlockTorch leads |
| `blocktorch/experimental` | FlashAttention kernels, `benchmarks/run.py`, `scripts`, `runs/`, `kernel_lib/` | Benchmarks referencing `/tmp/bench/<commit>/benchmarks.json` + `experimental/runs/<commit>` recorded; spec references note `USE_FLASH_ATTN=2`, `-DORCHARD_BUILD_EXPERIMENTAL=ON`, and tradeoffs. | Benchmarks/experimental owner |
| `node/src/compute` | `matcher.rs`, `receipt.rs`, `telemetry/receipts.rs`, `settlement.rs` | Receipt metadata includes kernel hashes, `ORCHARD_TENSOR_PROFILE` epochs, proof latency buckets; tests cover `replay`, `settlement_audit`, `fuzz_coverage`. | Compute lead |
| `governance` | `params.rs`, `launch_governor`, `config.rs` knobs | New knobs in `TB_BLOCKTORCH_*`, documented in docs + plan, chained to governor entries, shadow mode runbooks verified. | Governance owner |
| `metrics-aggregator` / `monitoring` | `/wrappers` config, Grafana JSON, aggregator telemetry | New metrics exported, dashboards refreshed via `npm ci --prefix monitoring && make monitor`, aggregator exposures enumerated, new panels include kernel hash label, `snark_prover_latency_seconds` gauge, `orchard_alloc_free_delta` histogram. | Telemetry ops |
| Explorer / CLI | `explorer/src/routes/receipts`, `cli/src/compute.rs`, `cli/src/governor.rs` | Explorer JSON includes BlockTorch receipts (kernel hash, proof latency, `benchmarks.json` link) + CLI status surfaces same info. | Explorer/CLI lead |

### 5.c Definitions & Key Terms
- **Metal Tensor + BlockTorch Kernel Hash** — metadata bundle (SHA256) covering the set of Metal or CUDA shader source files used by a job; exported in receipts under `kernel_variant.digest` and referenced by aggregator metrics/dashboards.
- **Proof verification budget** — the 100 ms upper bound for `snark_prover_latency_seconds` (reported per lane) measured from receipt emission to ledger settlement; failure triggers SLA slashing and a governor intent event. Settlement compares the recorded `ProofBundle::latency_ms` samples before applying `SlaOutcome::Completed`, warns if the max latency exceeds the budget and emits `SlaOutcome::Violated { reason: "proof_latency_budget" }`, so operators can monitor `tb-cli governor status`, `tb-cli compute stats`, and the `/wrappers` hash before touching the knob.
- **Allocator log epoch** — sequential record from `/tmp/orchard_tensor_profile.log` capturing `alloc/free` pairs with labels like `Tensor::fromData`, `runtime::metal_fill`, etc.; aggregator `orchard_alloc_free_delta` histogram calculates discrepancy per job.
- **Determinism lab log** — reproducibility artifact (structured JSON) combining the BlockTorch backend commit hash, `benchmarks/run.py` output path, kernel hash set, `ORCHARD_TENSOR_PROFILE` path, and driver/OS metadata; attached to each PR and stored as part of the DoD.
- **Gov shadow/apply** — each new parameter flows through a governor gate with two modes (shadow record only vs apply + state change); documented transitions appear in `docs/operations.md#telemetry-wiring` via table (Gate name, metric, prerequisites, rollback steps).
- **SDK plugin trait** — interface described in `node/src/compute/mod.rs` that any future fabric implements (init, run, proof emit, cost hint, telemetry hooks); documented here and referenced in `docs/subsystem_atlas.md`.
- **Experimental FlashAttention branch** — alternate execution path enabled via `USE_FLASH_ATTN=2` and `-DORCHARD_BUILD_EXPERIMENTAL=ON`; outputs pipeline through `experimental/runs/` plus aggregator metadata for `flash_attention_{throughput,latency}` metrics.

### 6. Future-Proofing (SDK Plugin Architecture)
- Document plug-in hook points (device selection, deterministic scheduler, proof emitter) in this plan and update `docs/subsystem_atlas.md` once modules land.
- Include checklist for future hardware: ensure any new plugin implements the standard interfaces, emits the same telemetry, and is validated through the plan’s DoD.
- Mirror every TODO/follow-up into `AGENTS.md §15` referencing this plan.

### 7. References
- `AGENTS.md §0.1`, `§0.2`, `§15` for spec-first rules and backlog directives.
- `docs/overview.md#document-map` for owner routing.
- `docs/architecture.md#compute-marketplace` and `#ledger-and-consensus` for existing flows.
- `docs/operations.md#telemetry-wiring` and `#troubleshooting-playbook` for telemetry and incident guidance.
- `docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md#part-xii` for BlockTorch strategy (to be patched as part of this plan).
