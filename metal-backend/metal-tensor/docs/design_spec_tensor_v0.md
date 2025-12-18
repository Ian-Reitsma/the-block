## Tensor v0 Design Specification

### 1. Repository Layout
- The project is organized as a mono-repo where all tensor sources live under `metal-tensor/` and legacy PyTorch code is quarantined in `experimental/`. This makes the tensor stack self-contained and avoids hidden dependencies on submodules.
- Static libraries are emitted from this tree: `liborchard_core.a` hosts CPU-only code and `liborchard_metal.a` contains Metal helpers. Both libraries are built from sources in this directory and exported to any higher-level consumers.

### 2. Build & Toolchain Requirements
- CMake ≥ 3.27 configures a C++20 / Objective-C++20 toolchain with `-std=gnu++20`, `-fno-objc-arc`, and `-ObjC++` flags. These settings guarantee uniform language features and ABI behavior across all targets.
- Fat binaries for `arm64;arm64e` are produced by default, ensuring the libraries run on every Apple Silicon generation. Developers must install Xcode Command Line Tools and the Metal SDK; the build scripts query `xcrun` to derive the correct SDK paths.
- Linux hosts lack the necessary compilers and Metal SDK. Agents must still run the configuration and build commands and record any failure output.

### 3. Tensor API Surface
- `Tensor.h`, `TensorImpl.h`, `Storage.h`, and `DType.h` form the public ABI. They expose rank-8 shape/stride arrays, intrusive reference counting, and a `Device` enum covering `cpu` and `mps` backends.
- Factory methods `empty`, `zerosLike`, and `fromData` are marked `[[nodiscard]]` so callers cannot accidentally drop tensors. Transformations such as `view`, `slice`, `to`, and `contiguous` always return new `Tensor` objects that own their metadata and maintain reference counts correctly.

### 4. Memory Model & Storage Semantics
- All allocations are 64-byte aligned. The CPU path uses `posix_memalign` while the Metal path delegates to `MTLBuffer` with `storageModeShared`. For buffers larger than 16 MiB an IOSurface is created and marked purgeable to survive memory pressure on macOS.
- `Storage` holds a UUID label, a pointer to the owning `Allocator`, and an atomic refcount. Move construction steals the pointer without touching the refcount; cloning allocates a new buffer and copies the contents so mutations never alias unexpectedly.

### 5. Runtime Contexts
- `CpuContext` wraps Accelerate/BNNS primitives. Each thread lazily constructs its own context and exposes helpers like vector addition so CPU fallback paths are straightforward to implement.
- `MetalContext` wraps the system default `MTLDevice` and maintains a thread-local pool of `MTLCommandQueue` objects. Returning queues to the pool amortizes creation costs and keeps GPU dispatch lock-free.

### 6. Autograd Hooks & Forward/Backward Strategy
- `TensorImpl` reserves `grad_fn` and `grad_ctx` pointers. Initial autograd scaffolding is present; gradients for elementwise add are validated across CPU and Metal. The full, general engine is in progress and will expand coverage beyond add.
- The long-term goal is a **full Metal-native backward pass**. As coverage grows, both forward and backward kernels will run entirely on Metal without detouring through PyTorch, unlocking higher throughput on Apple Silicon.
- The existing FlashAttention forward pass continues to work through `libflash_attn.dylib`. Build scripts for the dylib include headers from `metal-tensor/`, and a shim converts `orchard::Tensor` to `at::Tensor` during the transition period.

### 7. Validation Matrix & Testing Strategy
- Unit tests live under `metal-tensor/tests/` and use a vendored copy of GoogleTest under `third_party/googletest`. They verify zero-copy `to("cpu")`, slice/view mutation coherence, clone storage independence, data pointer alignment, and allocator stress with 100 k alloc/free cycles.
- Fuzz tests and race-condition tests (planned) will exercise random shape/stride transformations and multi-threaded reference counting. Any imbalance or lock misuse fails the suite to keep concurrency bugs from creeping in.
- Before every pull request, run `cmake --build build --target check` on a configured build tree and capture success or failure in the pull request description.

### 8. Instrumentation & Profiling Hooks
- When compiled with `ORCHARD_PROFILE_ALLOC`, every allocation logs its UUID, shape, dtype, and device to `/tmp/orchard_tensor_profile.log`. These logs feed directly into Instruments or custom analysis scripts for leak tracking.
- Each `MTLBuffer` is tagged `tensor-<UUID>` so the Metal Performance HUD and GPU capture tools can correlate GPU memory to tensor metadata. A debugging function `dump_live_tensors()` enumerates non-zero refcounts for post-mortem analysis.

### 9. Documentation & Developer Workflow
- `docs/tensor.md` describes API usage, memory model diagrams, and profiling instructions. Contributors follow `AGENTS.md` for the canonical build and test workflow, which no longer references submodules.
- All public headers must compile standalone under `-Wall -Wextra -Wpedantic`, and code is formatted with `clang-format`. Pull requests that fail to run `cmake` and the test suite locally are rejected.
- Documentation and commit messages reference identifiers using inline code and avoid fenced code blocks. Use `rg` for searches and refrain from `grep -R` or `ls -R`.

### 10. Roadmap & Deprecation Plan
- Phase 1 stabilizes Tensor v0, allocator, runtime contexts, and validation tests while keeping the PyTorch bridge behind `ORCHARD_BUILD_EXPERIMENTAL`. Once this phase is complete the forward pass of FlashAttention will be recompiled to consume `orchard::Tensor` directly.
- Phase 2 adds foundational ops and broad autograd coverage, enabling full training loops without PyTorch. The legacy bridge will be deprecated and eventually removed once performance parity is demonstrated across macOS 13–15 on Apple M-series hardware.
