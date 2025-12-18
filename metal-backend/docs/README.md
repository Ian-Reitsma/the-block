# Documentation

The `docs` directory collects narrative material that spans the entire project. Each file explains the state of the effort or elaborates on a design facet.

## Contents
- `project_status.md` records milestones, active workstreams, and upcoming tasks for both the experimental PyTorch bridge and the Metal-native tensor path.
- `tensor.md` introduces Tensor v0, describes the toolchain requirements, and outlines features such as zero-copy transfers and allocation profiling.
- `../benchmarks` houses scripts that capture hardware details, runtime flags, and kernel timings.

## Current Status
 - `project_status.md` and `tensor.md` are current as of August 2025 and chart both the experimental bridge and the Metal-native tensor implementation.
   - Recent updates describe elementwise division with broadcast-aware zero masking that recomputes denominator offsets each step, constant tensor filling, explicit detachment, the Metal mean kernel, and the CPU-only fallback documented in the Toolchain section where Metal discovery is gated on `CMAKE_SYSTEM_NAME` and a stub `Metal::Metal` target unblocks non-Apple hosts. The Metal allocator now falls back to host memory when Metal APIs are unavailable, `metal/runtime/runtime_cpu.cpp` implements this path, Objective-C++ sources are skipped on CPU builds, and profiling events continue to stream. Autograd nodes snapshot inputs before in-place scalar divisions to block recursive gradients with regression tests for chained and repeated `div_` calls and CPU add and mul backward paths, and backward consumes the saved tensor so gradients accumulate once. Transpose backward routes gradients through the CPU kernel when tensors reside on the host and dispatches Metal kernels otherwise, forwarding a freshly transposed gradient tensor upstream. Broadcast tests align shapes to avoid crashes, sum and mean recompute shapes and strides when dimensions drop or are kept so axis 1 reductions stay accurate, and profiling may be toggled at runtime by changing `ORCHARD_TENSOR_PROFILE`, `tensor_profile_reset` refreshes the cached flag, and `tensor_profile_clear_log` removes stale logs.
 - Allocation profiling emits one `alloc` line when storage is created and a matching `free` line when the final reference drops so logs stay symmetric even when underlying memory is pooled, and stress tests confirm counts remain balanced under queue reuse.
 - CPU-only builds run transpose, matmul, mean, and sum backward tests to widen autograd coverage when Metal kernels are absent, expanding beyond division.
- Design specifications under `metal-tensor/docs/` provide deeper notes on kernels, runtime contexts, and autograd scaffolding.
- Documentation is actively maintained yet lacks full API references and architecture diagrams.

## Milestones
1. Publish a complete API reference for Tensor v0 covering public headers and usage semantics.
2. Add diagrams illustrating memory flows, command queues, and autograd graphs.
3. Establish a changelog that records major documentation updates alongside code milestones.

## Next Steps
1. Capture profiling and debugging tutorials that guide new contributors through common workflows.
2. Document the FlashAttention migration plan from the experimental bridge to Tensor v0.
3. Cross-link design notes and tutorials so related topics remain easy to navigate.

## Benchmarks
Scripts in `../benchmarks` record hardware information, runtime flags beginning with `ORCHARD_`, kernel timings, and whether Metal or CPU kernels executed. When `ORCHARD_TENSOR_PROFILE` is set the harness also embeds allocator profiling lines from `/tmp/orchard_tensor_profile.log`. Invoke `python benchmarks/run.py -o /tmp/bench` after building to generate a JSON file under `/tmp/bench/<commit>/benchmarks.json`; set `ORCHARD_FORCE_CPU=1` to capture CPU-only runs. Results are untracked, and the `Benchmarks` workflow can be triggered manually to archive the JSON. Each JSON entry logs add, mul, matmul, reduce_sum, mean, and transpose timings alongside system metadata. See `../AGENTS.md` for the repository benchmark protocol.

## Guidelines
- Expand these documents whenever significant features land or roadmap items change.
- Reference identifiers with inline code and avoid fenced code blocks.
- Ensure any added file fits within the repository’s 5 MB artifact limit.
- Note that tests use the trimmed `third_party/googletest` tree or a system package when `FETCHCONTENT_FULLY_DISCONNECTED=ON` is supplied during configuration so documentation referencing the test workflow should mention this offline mode.

## Contributor Protocol
- Read `../AGENTS.md` before editing documentation; it describes required build and test steps and repository etiquette.
- Run `cmake -S . -B build -G Ninja` and `cmake --build build --target check` from the repository root prior to opening a pull request; capture any failing output.
- Search with `rg` instead of recursive `ls` or `grep`.
- When profiling behaviour is exercised in tests, toggle `ORCHARD_TENSOR_PROFILE` with `tensor_profile_reset` and clear `/tmp/orchard_tensor_profile.log` via `tensor_profile_clear_log`.
- Keep documentation free of fenced code blocks; use inline code to reference commands and identifiers.
- Commit messages must be a single imperative line and pull requests should reference modified files by path and line number.
- Do not commit generated files or assets larger than five megabytes.
