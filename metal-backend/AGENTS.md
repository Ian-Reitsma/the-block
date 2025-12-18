# AGENTS.md – Command and Coordination Manual

This manual governs every action within the `metal-orchard` repository. The project rebuilds a Metal-centric tensor runtime and kernel stack known internally as Tensor v0. All source code, tests, and documentation reside in this repository; no submodules or external trees exist. Every contributor must treat this document as the authoritative source of truth when modifying any file.

## AGENTS File Inventory
Automated agents must locate every `AGENTS.md` before editing files. Enumerate them from the repository root with `find . -name AGENTS.md -print` so no directory-level guidance is missed.

As of this revision, these files exist:
- `/AGENTS.md` – global policies for the entire tree.
- `docs/AGENTS.md` – documentation directory guidance.
- `metal-tensor/AGENTS.md` – core tensor library guidance.
Introduce additional `AGENTS.md` files in new directories and update this inventory accordingly.

## Repository Cartography
The directory layout is intentionally shallow to make navigation unambiguous:

- `metal-tensor/` – hosts the core library. Inside this directory:
  - `metal/` contains the production source. It is subdivided into `common/` for utilities such as `Profiling.h`, `core/` for primary abstractions like `Tensor` and `Storage`, `kernels/` for Metal shader entry points, and `runtime/` for queue management and device utilities.
  - `tests/` includes the comprehensive suite exercising contiguity handling, device transfer paths, allocation profiling, and autograd validation. The primary entry file is `tensor_tests.cpp`.
  - `docs/` records design specifications such as `design_spec_tensor_v0.md`.
- `experimental/` – preserves the legacy PyTorch-based path. Subdirectories include `orchard_ops/` for C++ and Python extension modules, `benchmarks/` for performance scripts, `tests/` for PyTorch-driven verification, `kernel_lib/` for prebuilt FlashAttention binaries, and transient holders like `data/` and `runs/` which remain ignored by Git.
- `docs/` – houses project-wide narrative material.
- `.github/` – contains continuous integration workflows: `workflows/macos.yml` for macOS and `workflows/linux.yml` for CPU-only builds.
- `third_party/` – vendors external code. A trimmed `googletest` tree supplies headers and sources only; upstream tests, samples, and documentation were dropped to keep the repository small.

## Component Highlights
- `Storage` implements intrusive reference counting to track underlying `MTLBuffer` allocations. The static factory `Tensor::fromData` wraps external memory without copying by accepting a raw pointer, explicit shape, data type, device, and optional deleter callback.
- `Tensor::to` performs host and device transfers. When the source and destination `Device` values match, the call resolves to a zero-copy view preserving the original storage.
- Allocation profiling is governed by `metal/common/Profiling.h`. Setting the environment variable `ORCHARD_TENSOR_PROFILE` enables logging of alloc, free, and live events. The diagnostic helper `dump_live_tensors` enumerates outstanding `Storage` instances to standard error for post-mortem analysis.
- Autograd is scaffolded through `Tensor::requires_grad`, the gradient tensor accessible via `Tensor::grad`, and a graph of `Node` and `Edge` objects culminating in the `backward` routine.
- Metal compute kernels cover elementwise add and multiply, matrix multiply, and reduce_sum. Shaders live under `metal/kernels/` and dispatch one thread per output element. CPU fallbacks in `metal/core/` activate when Metal is unavailable.

## Build Protocol
1. Obtain Xcode 15+ with the Metal 4 SDK; ensure command line tools are active. Builds only query the Metal SDK when `CMAKE_SYSTEM_NAME` is `Darwin` and `FindMetal.cmake` registers a stub `Metal::Metal` target elsewhere so CPU-only hosts can proceed without the SDK.
2. From the repo root, configure with CMake into a `build/` dir using Ninja. Pass `-DFETCHCONTENT_FULLY_DISCONNECTED=ON` to keep configuration offline and rely on the trimmed `third_party/googletest` tree or a system package; optionally enable `-DORCHARD_BUILD_EXPERIMENTAL=ON` to compile the legacy PyTorch bridge under `experimental/`.
3. Build the default target. Darwin emits `liborchard_core.a` and `liborchard_metal.a`; other platforms produce only `liborchard_core.a` as a CPU fallback.

## Test Protocol
1. With a configured build tree, run the `check` target. Tests live under `metal-tensor/tests/` and exercise CPU/Metal paths.
2. Run configure + tests before every PR and capture failure logs in the PR description when toolchains are missing.
3. When `FETCHCONTENT_FULLY_DISCONNECTED=ON` is set during configuration the `metal_tensor_tests` target links against the trimmed `third_party/googletest` tree or a system installation so the suite executes without network access.
4. Tests that flip `ORCHARD_TENSOR_PROFILE` must call `tensor_profile_reset` after changing the environment and purge `/tmp/orchard_tensor_profile.log` with `tensor_profile_clear_log` to keep logs isolated.

## Benchmark Protocol
- Invoke `python benchmarks/run.py -o /tmp/bench` after building to record kernel timings and hardware metadata. Results land under `/tmp/bench/<commit>/benchmarks.json`.
- Generated JSON results remain untracked; the Benchmarks workflow uploads them as artifacts. See `docs/README.md` for interpreting outputs.

## Contribution Directives
- C++20 and ObjC++ only; format with `clang-format`.
- Do not commit artifacts larger than 5 MB. Large datasets belong under `experimental/data/` or `experimental/runs/` and remain untracked.
- Documentation/commits avoid code blocks; refer to identifiers by name (e.g., `Tensor::fromData`, `ORCHARD_TENSOR_PROFILE`, `flash_attn`).
- Always run configure + tests locally before PRs.
- Use `rg` for repository searches and avoid `ls -R` or `grep -R` to keep scans efficient.
- Commit messages use the imperative mood and include a short summary line only.
- Capture the output of `cmake -S . -B build -G Ninja` and `cmake --build build --target check` and report failures in the pull request.
- Reference touched files by path and line number in pull request descriptions.
- Work exclusively on the default branch and refrain from creating new branches within this repository.
- Keep the CI matrix green. macOS runners for `macos-13` and `macos-14` must pass; the Linux diagnostic job may fail but its logs require review before merging.
- Every directory containing a `README.md` must expose a `Contributor Protocol` section that points back to this manual. Whenever repository guidelines change, update every README to keep that section in sync.

## Workflow Checklist
1. Run `cmake -S . -B build -G Ninja` from the repository root.
2. Invoke `cmake --build build --target check` and note any errors.
3. Stage changes with `git add` and create a single commit per task.
4. Formulate a pull request summarizing the intent, the files modified, and the test outcomes.

## Documentation Discipline
- Treat this file as the canonical reference for repository policy. Read it in full before editing any file.
- README files and documents under `docs/` must mirror the current guidance. When you revise instructions here, propagate equivalent wording to all README files in the tree.
- Use inline code for commands and identifiers; never introduce fenced code blocks in documentation or commit messages.
- Cite file paths and line numbers in pull requests so reviewers can locate changes quickly.
- Omit placeholder text. If a task cannot be completed, mark incomplete work with a TODO comment and document the limitation in the pull request.

## Current Status
- Tensor v0 now layers elementwise division, constant filling, and explicit detachment on top of intrusive storage and host and device transfer paths.
- Division backward dispatches by device, allowing CPU-only builds without Metal.
- The Metal allocator falls back to host memory on non-Apple platforms while
  still emitting profiling events for allocations and frees.
- Autograd nodes retain input tensors to avoid recursive gradient application;
  regression tests cover chained in-place scalar divisions and CPU add and mul
  backward paths.
- Vector and matrix broadcast tests now align shapes, resolving a prior crash.
- Detached views share storage; `Tensor::is_alias_of` verifies aliasing and tests mutate through detached tensors and clone-before-detach paths.
- A dedicated Metal kernel drives `Tensor::mean`, removing the final CPU post-processing step and aligning performance across devices.
- Benchmarks cover add, mul, matmul, reduce_sum, mean, and transpose and log results beneath commit-specific directories for reproducible performance tracking.
- The legacy PyTorch bridge persists under `experimental/` but is excluded from default builds.
- Continuous integration now covers `macos-13` (M1) and `macos-14` (M2) with Xcode 15.3 pinned and Homebrew updates disabled. A Linux job installs a clang-based Objective-C++ toolchain and is allowed to fail for diagnostics.
- Documentation outlines tensor internals, profiling hooks, and contributor expectations yet remains a living reference.
- GoogleTest ships as a minimal vendored copy under `third_party/googletest` so tests compile without network access; obtain upstream tests and samples from the official repository when needed.
  - Safe division recomputes denominator offsets after each broadcast step so
    zero denominators do not poison later elements.
  - Sum and mean shift stride metadata after axis reductions so dimension 1
    yields correct shapes and values whether `keepdim` is true or false.
  - Autograd nodes snapshot inputs before in-place scalar divisions so
    backward uses pre-mutation values and gradients accumulate once even across
    chained or repeated `div_` calls.
  - Transpose backward dispatches a CPU kernel for host tensors and forwards a
    freshly transposed gradient tensor upstream; Metal kernels are used when
    inputs reside on the device.
  - Profiling reads `ORCHARD_TENSOR_PROFILE` on every query and pairs each
    `alloc` with a matching `free` entry even under load; `tensor_profile_reset`
    forces the flag to refresh between runs and `tensor_profile_clear_log`
    deletes stale log files.

## Milestones
1. Phase out the PyTorch bridge once Tensor v0 covers FlashAttention and essential autograd features.
2. Ship a fully Metal-native backward pass with fused FlashAttention kernels and dropout support.
3. Deliver a public benchmarking suite with reproducible configurations and published baselines.
4. Cut a 0.1 release that freezes the API and tags the first feature-complete snapshot.

## Next Steps
1. Fix CMake configuration on non-Apple platforms so CPU-only builds succeed without Objective-C++.
2. Extend differentiable operations to include additional elementwise and reduction primitives beyond add, mul, div, and mean.
3. Replace remaining CPU fallbacks with tuned Metal kernels and delete redundant host code.
4. Grow the test matrix to cover multi-device transfers, profiling scenarios, and stress conditions.
5. Stand up a benchmarking harness that records hardware, runtime flags, and kernel timing for every commit while archiving JSON outputs per commit.
6. Broaden the test matrix for additional CPU-only execution paths and stress scenarios.
