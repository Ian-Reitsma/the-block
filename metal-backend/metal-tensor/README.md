# metal-tensor

`metal-tensor` contains the Tensor v0 library and runtime. It provides:

- intrusive ref-counted Storage objects
- Tensor::empty, Tensor::view, Tensor::slice, and zero-copy Tensor::fromData
- CPU and Metal transfers via Tensor::to
 - allocation profiling and dump_live_tensors for debug tracing; `ORCHARD_TENSOR_PROFILE` is cached after first use, `tensor_profile_reset` refreshes the flag between tests, and `tensor_profile_clear_log` removes stale logs
- a starter autograd engine with Tensor::requires_grad, gradient tensors, and Node and Edge graph powering backward for matmul, reductions, elementwise add and multiply, division, transpose, and view
- Metal compute kernels covering vector add, matmul, whole-tensor reductions, and a dedicated mean kernel, automatically selected when tensors live on an mps device
- constant filling through Tensor::fill and storage detachment with Tensor::detach

## Current Status
- CPU and Metal backends allocate tensors with intrusive storage and share views without copying.
- Host and device transfers through Tensor::to round-trip data between CPU and mps devices.
- Tests validate contiguity, profiling logs, command-queue pooling, multi-device transfers, alignment on non-contiguous views, constant filling, detachment semantics, and gradient propagation for elementwise add, multiply, divide, matmul, mean, reductions, transpose, and view transforms.
- The Metal allocator falls back to host memory when Metal APIs are unavailable while still logging profiling events.
  `metal/runtime/runtime_cpu.cpp` provides this path and builds when Objective-C++ sources are excluded.
- Autograd nodes snapshot inputs before in-place scalar divisions to avoid recursive gradient application with regression tests for single, repeated, and chained `div_` calls alongside CPU add and mul backward paths, and backward consumes the saved tensor so gradients reference the original values and accumulate once.
- Vector and matrix broadcast tests now align shapes to prevent prior crashes.
 - Safe division recomputes denominator offsets after each broadcast step so zero denominators do not poison later elements.
 - Sum and mean recompute output shapes and strides after axis reductions so dimension 1 yields correct shapes and values.
- Transpose backward routes gradients through the CPU kernel for host tensors and dispatches Metal kernels otherwise, forwarding a freshly transposed gradient tensor to upstream nodes.
 - Profiling caches `ORCHARD_TENSOR_PROFILE` after first use and pairs every allocation with a matching free; `tensor_profile_reset` refreshes the flag between runs and stress tests assert counts remain equal under queue pooling.
- CPU-only builds run autograd regressions for transpose, matmul, mean, and sum so gradients remain validated without Metal kernels, expanding coverage beyond division.
- Vector add and multiply, division, matmul, mean, and full reductions ship with Metal kernels for forward and backward paths, and transpose backward dispatches CPU or Metal kernels based on device.

## Directory map
- `metal/` holds the implementation source
  - `common/` groups utilities such as Profiling.h and debug helpers including dump_live_tensors
  - `core/` defines Tensor, Storage, Device, and factory functions like Tensor::empty, view, slice, and fromData
  - `kernels/` contains Metal shader entry points
  - `runtime/` manages MTLDevice selection and command queue pooling referenced by Tensor::to during transfers
- `tests/` bundles unit tests centred around tensor_tests.cpp that validate contiguity, CPU arithmetic, Metal dispatch, profiling, and autograd
- `docs/` houses design specifications such as design_spec_tensor_v0.md

## Building
1. Ensure Xcode 15+, the Metal 4 SDK, and command line tools are installed.
2. From the repository root run `cmake -S . -B build -G Ninja` followed by `cmake --build build` to configure and build the library. CMake consults the Metal SDK only when `CMAKE_SYSTEM_NAME` is `Darwin`; other platforms receive a stub `Metal::Metal` target, skip Objective-C++ sources, and compile `metal/runtime/runtime_cpu.cpp`. Supplying `-DFETCHCONTENT_FULLY_DISCONNECTED=ON` during configuration directs tests to the trimmed `third_party/googletest` tree or a system package and keeps the process offline.
3. Non-Apple hosts follow the same steps and produce only `liborchard_core.a`; include any diagnostic output in pull requests.

## Testing
Invoke the tests with `cmake --build build --target check`. The suite covers contiguity, CPU arithmetic, safe division masking, sum and mean parity on CPU and Metal, command-queue pooling, multi-device CPU↔Metal↔CPU transfers, mixed CPU→Metal→CPU→Metal sequences, multi-threaded large tensor moves, profiling log validation with matching alloc/free counts, silent behaviour when `ORCHARD_TENSOR_PROFILE` is unset, alignment and zero-copy checks, and autograd gradients for elementwise add, multiply, and transpose.
CPU-only configurations also run transpose, matmul, mean, and sum backward tests so gradients remain verified without Metal.

## Milestones
1. Autograd support for a base operator set including matmul and reductions.
2. Optimized Metal kernels for all core operations with parity to CPU fallbacks.
3. Stable 0.1 release enabling external projects to consume `liborchard_core.a` and `liborchard_metal.a`.

## Next Steps
- Broaden autograd coverage and add more differentiable operators.
- Implement Metal kernels for remaining CPU paths and retire redundant code.
- Grow the test suite to cover additional device transfers and upcoming ops.

## Contributor Protocol
- Study `../AGENTS.md` before changing any source, test, or documentation file; it governs every operation in this repository.
- Run `cmake -S . -B build -G Ninja` and `cmake --build build --target check` from the repository root prior to committing, and capture any failure output.
- When profiling tests modify `ORCHARD_TENSOR_PROFILE`, call `tensor_profile_reset` after each change and remove stale logs with `tensor_profile_clear_log`.
- Use `rg` to search for symbols; avoid recursive `ls` or `grep` invocations.
- Format C++20 and Objective-C++ code with `clang-format` and keep lines concise.
- Never commit generated files, build artifacts, or assets larger than five megabytes.
- Limit each pull request to a single commit with an imperative summary line and cite modified files by path and line number.
- Operate solely on the default branch and leave the worktree clean after your commit.
