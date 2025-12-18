# Metal Orchard

`metal-orchard` is the incubation ground for Tensor v0, a tensor runtime and kernel stack engineered for Apple Silicon and the Metal application programming interface. The repository hosts every source file, test, and document required to construct the project; no external submodules are referenced.

## Getting Started
1. Install Xcode 15+, the Metal 4 SDK, and the command line tools so the build system can locate compilers and headers.
2. Configure the project with `cmake -S . -B build -G Ninja` to generate build files in a separate `build/` directory.
3. Compile targets by running `cmake --build build` which produces static libraries and the test binary.
4. Execute `ctest --output-on-failure` inside `build/` to verify the runtime passes its unit tests.

## Repository Overview
- `metal-tensor/` contains the primary library. Its `metal/` tree defines `Storage`, `Tensor`, `Node`, and auxiliary infrastructure, while `tests/` verifies contiguity semantics, host and device copies through `Tensor::to`, allocation profiling via `dump_live_tensors`, and gradient propagation using the `backward` routine.
- `experimental/` carries the historical PyTorch bridge. Its `orchard_ops/` folder builds C++ and Python extension modules, `benchmarks/` and `tests/` exercise them under PyTorch, and `kernel_lib/` stores prebuilt FlashAttention binaries. The `data/` and `runs/` directories hold transient datasets and benchmark outputs and remain untracked by Git to avoid committing large artifacts. The bridge is disabled by default and only compiles when configuration passes -DORCHARD_BUILD_EXPERIMENTAL=ON and runtime sets USE_FLASH_ATTN to 2.
- `docs/` collects narrative material including design notes and project status reports.
- `.github/` defines the continuous integration pipeline in `workflows/macos.yml`.
- The repository root hosts `CMakeLists.txt` for configuring all targets and a `build/` directory is created by contributors to hold generated files.

## Feature Highlights
- Rank-eight shape representation with explicit stride control for advanced view and slice operations.
- Intrusive reference counted `Storage` objects that permit zero-copy wrapping of external buffers through `Tensor::fromData`.
- Host and device transfers mediated by `Tensor::to`, yielding zero-copy aliases when the destination `Device` matches the source.
  - Allocation profiling managed by `metal/common/Profiling.h`. When `ORCHARD_TENSOR_PROFILE` is present in the environment, allocation and release events stream to `/tmp/orchard_tensor_profile.log`, and `dump_live_tensors` reports outstanding buffers. The flag is cached after the first query, `tensor_profile_reset` refreshes the state between tests, and `tensor_profile_clear_log` removes stale log files.
- The Metal allocator falls back to host memory on non-Apple platforms while preserving allocation and free profiling logs.
  `runtime_cpu.cpp` implements this fallback and compiles when Objective-C++ sources are omitted.
- Autograd foundations supplied by the `requires_grad` flag, gradient accumulation in `Tensor::grad`, and dedicated nodes for matmul, reductions, view, elementwise add and multiply, transpose, and division. `Tensor::detach` returns a view that shares storage but halts gradient propagation.
- Autograd nodes snapshot inputs before in-place scalar divisions so backward uses pre-mutation values and gradients accumulate once; regression tests cover single, repeated, and chained in-place `div_` sequences and CPU add and mul backward paths.
- Initial Metal compute kernels, located under `metal-tensor/metal/kernels/`, implementing vector addition, matmul, whole-tensor reductions, and a dedicated mean kernel; each operation automatically falls back to CPU code when Metal execution is unavailable.
- Constant filling through `Tensor::fill` sets every element to a value on both CPU and Metal devices.
- `Tensor::div` checks denominators for zero and can mask them when a safe flag
  is provided; see [docs/tensor.md](docs/tensor.md#elementwise-division) for
  details.

## Testing
Run `ctest --output-on-failure` from the `build/` directory to execute the suite under `metal-tensor/tests`. The tests cover CPU and Metal paths, queue reuse, profiling hooks with matching alloc/free counts, and autograd gradients. CPU-only builds exercise transpose, matmul, mean, and sum backward paths to validate gradients without Metal. Always attempt to configure and run tests before submitting a pull request. Even on systems lacking the Metal SDK, failing output is still valuable and should be reported in the pull request.

## Benchmarking
Invoke `python benchmarks/run.py -o /tmp/bench` after building to capture kernel timings, hardware details, runtime flags, and whether Metal or CPU kernels executed. When `ORCHARD_TENSOR_PROFILE` is set the harness embeds allocator profiling lines from `/tmp/orchard_tensor_profile.log`, and setting `ORCHARD_FORCE_CPU=1` forces CPU-only runs. Results are written to `/tmp/bench/<commit>/benchmarks.json` where `<commit>` is the short Git hash. The harness exercises addition, multiplication, matmul, reduce_sum, mean, and transpose and enables reproducible comparisons across commits.

## Autograd Notes
Tensors opt into gradient tracking through the requires_grad property. Operations such as Tensor::add, Tensor::mul, Tensor::div, Tensor::matmul, Tensor::sum, Tensor::mean, Tensor::transpose, and Tensor::view register Node instances connected by Edge relationships. Calling backward performs a reverse traversal to populate Tensor::grad on leaf tensors. `Tensor::detach` returns a view that shares storage but discards autograd state and prevents gradients from flowing through the new tensor. Mutating the detached tensor reflects in the source due to shared storage; `Tensor::is_alias_of` reports whether two tensors reference the same buffer. A CPU tensor built with `Tensor::fromData` can call `detach` and confirm aliasing through `Tensor::is_alias_of`. Clone a tensor before detaching when an isolated copy is required to avoid unintended writes. See docs/tensor.md#detaching-tensors for additional guidance.

## Continuous Integration
The macOS workflow described in .github/workflows/macos.yml installs dependencies through Homebrew, configures the project with the Ninja generator, treats warnings as errors, and executes the full test suite. Build artifacts and ccache directories are cached to accelerate subsequent runs. Any warning or failing test causes the pipeline to halt.

## Profiling Guidance
Setting ORCHARD_TENSOR_PROFILE enables logging of allocation and deallocation events along with explicit dumps triggered by dump_live_tensors. The variable is cached on first use so call tensor_profile_reset after changing the environment, and logs accumulate at /tmp/orchard_tensor_profile.log for offline inspection that tensor_profile_clear_log can remove.

## Current Status
- Tensor v0 handles intrusive storage, host and device transfers, elementwise division with zero masking, constant filling, explicit detachment, and validated gradients for matmul, reductions, view, elementwise addition and multiply, division, and transpose.
 - Safe division recomputes denominator offsets after each broadcast step so zero denominators do not poison later elements.
 - Sum and mean recompute output shapes and strides after axis reductions so dimension 1 yields correct shapes and values.
- Transpose backward routes gradients through CPU kernels for host tensors and dispatches Metal kernels otherwise, forwarding a freshly transposed gradient tensor to upstream nodes.
 - Profiling caches `ORCHARD_TENSOR_PROFILE` after first use and emits symmetric `alloc` and `free` pairs even when memory is pooled; `tensor_profile_reset` forces the flag to refresh between runs and a stress test confirms counts stay balanced.
- CPU-only builds run autograd regressions for transpose, matmul, mean, and sum so gradients remain validated without Metal kernels, expanding coverage beyond division.
- The PyTorch bridge under `experimental/` remains available for regression checks but is omitted from standard builds.
- macOS continuous integration enforces warnings-as-errors and executes the test suite; Linux hosts provide diagnostic failures only.
- Fused FlashAttention backward kernels with dropout are present under `experimental/` and enable end-to-end gradient checks for keys and values.
- Documentation covers design specifications, tensor features, and project status but evolves with each milestone.
- A minimal copy of GoogleTest under `third_party/googletest` builds the test suite without fetching from the network; upstream tests and samples are not included.

## Milestones
1. Fused FlashAttention backward kernels with dropout support.
2. Removal of the PyTorch bridge once Tensor v0 reaches feature parity.
3. Public benchmarking harness with published baselines across Apple Silicon generations.
4. Versioned 0.1 release capturing the first stable API.

## Next Steps
1. Extend the differentiable operator set beyond add, mul, div, mean, and transpose.
2. Replace CPU fallbacks with tuned Metal kernels and document performance gains.
3. Grow the test matrix for multi-device transfers, profiling scenarios, and stress tests.
4. Iterate on FlashAttention kernels to close remaining gaps with the PyTorch baseline.

## Contributor Protocol
- Read `AGENTS.md` in this directory before touching any file; it is the definitive governance document.
- Configure the project with `cmake -S . -B build -G Ninja` from the repository root and capture all configure output.
- Run `cmake --build build --target check` and include any failure logs in pull requests, even on systems lacking the Metal toolchain.
- When toggling `ORCHARD_TENSOR_PROFILE` in tests, invoke `tensor_profile_reset` after changing the environment and delete stale `/tmp/orchard_tensor_profile.log` with `tensor_profile_clear_log`.
- Search the tree with `rg` instead of recursive `ls` or `grep` commands.
- Format C++20 and Objective-C++ sources using `clang-format`.
- Avoid committing generated files or artifacts exceeding five megabytes; build outputs belong under untracked directories such as `build`.
- Use a single commit per task with an imperative one-line summary.
- Reference modified files by relative path and line number in the pull request message so reviewers can inspect changes quickly.
- Work only on the default branch and leave the worktree clean when the commit is complete.
