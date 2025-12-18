# Experimental Code

The `experimental` directory preserves the legacy PyTorch bridge used before Tensor v0 matured. None of these files are required for the core build; the entire tree is quarantined and only compiled when explicitly requested.

## Directory map
- `orchard_ops/` contains C++ and Python extension modules that interface with PyTorch.
- `benchmarks/`, `scripts/`, and `tests/` provide harnesses and verification suites driven by PyTorch.
- `kernel_lib/` stores prebuilt FlashAttention dynamic libraries consumed by the extensions.
- `data/` and `runs/` act as drop zones for datasets and benchmark outputs. Both paths are ignored by Git so large artifacts never enter version control.

## Usage notes
- The bridge is excluded from normal builds. Configure with -DORCHARD_BUILD_EXPERIMENTAL=ON to enable it.
- FlashAttention kernels activate when the environment variable USE_FLASH_ATTN is set to 2 and the flash_attn extension is available.
- A complete PyTorch environment is required to compile and run any code in this subtree.

## Current Status
- Forward FlashAttention kernels run for GPT-2 attention layers when `USE_FLASH_ATTN=2`. A fused backward kernel now applies the dropout mask, rescales gradients, and produces outputs for query, key, and value tensors.
- The Python wrapper validates `head_dim` as a multiple of eight and enforces `dropout_p` within `[0,1)`, returning the mask alongside the attention output.
- Benchmarks serve regression comparison only and are not optimized for new features.
- Linux hosts can compile the extensions but execute them without Metal acceleration, limiting validation to CPU results.

## Milestones
1. Maintain compatibility with PyTorch 2.x until Tensor v0 absorbs all critical functionality.
2. Track performance deltas as Tensor v0 kernels replace these experimental implementations.
3. Remove the subtree once the Metal-native stack provides equivalent coverage.

## Next Steps
1. Replace the stubbed backward kernel with a fully fused Metal implementation producing true gradients for `q`, `k`, and `v`.
2. Periodically rerun benchmarks to detect drift between Tensor v0 and the PyTorch baseline.
3. Trim obsolete scripts and data to keep the directory lean until deprecation.

## Deprecation
These components remain only for historical comparison. They will be removed once the Metal stack reaches feature parity and no longer relies on PyTorch.

## Contributor Protocol
- Follow `../AGENTS.md` for all repository rules even when working in this experimental subtree.
- Configure and test the project with `cmake -S . -B build -G Ninja` and `cmake --build build --target check`; include any failures in pull requests.
- Pass `-DFETCHCONTENT_FULLY_DISCONNECTED=ON` when configuring without network access so tests link against the trimmed `third_party/googletest` tree or a system package.
- Use `rg` for code searches; avoid recursive `ls` or `grep` commands.
- Do not commit datasets, run outputs, or other generated files. The `data/` and `runs/` directories remain untracked to keep large artifacts out of version control.
- Format C++ and Python sources consistently and keep commits to a single logical change with an imperative summary line.
- Reference modified files by path and line number in the pull request message and work only on the default branch.
- When profiling behaviour is exercised, call `tensor_profile_reset` after
  toggling `ORCHARD_TENSOR_PROFILE` and remove stale logs with
  `tensor_profile_clear_log`.
