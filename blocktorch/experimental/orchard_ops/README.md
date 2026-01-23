# orchard_ops

This directory hosts all Python and C++ extension modules used by the legacy
PyTorch integration. Operators defined here are loaded as part of the
experimental pipeline and are **not** required for the standalone Metal tensor
stack.

## Contents
- custom kernels built as PyTorch extensions
- Python glue code for integrating those kernels

## Next Steps
These files remain for reference while the Metal stack matures. Remove or update
them once equivalent Metal-native kernels exist in `metal-tensor` and the
PyTorch path is no longer used.

Current additions include a dropout-aware FlashAttention backward launcher and an autograd wrapper that dispatches to the fused Metal kernel. These changes keep the experimental path aligned with Tensor v0 while development continues.

## Contributor Protocol
- Adhere to `../../AGENTS.md` when modifying extension code or Python glue.
- Run `cmake -S . -B build -G Ninja` and `cmake --build build --target check` from the repository root; record any failures in the pull request.
- When offline, configure with `-DFETCHCONTENT_FULLY_DISCONNECTED=ON` so tests link against the trimmed `third_party/googletest` tree or a system installation without downloading.
- Use `rg` for symbol searches; do not rely on `ls -R` or `grep -R`.
- Avoid committing compiled extensions, generated outputs, or files larger than five megabytes.
- Keep each commit focused on one change with an imperative summary line and cite modified files by path and line number in the pull request.
- Contribute on the default branch only and maintain a clean working tree after the commit.
- When profiling behaviour is exercised, reset the cached flag with
  `tensor_profile_reset` after changing `ORCHARD_TENSOR_PROFILE` and clear
  `/tmp/orchard_tensor_profile.log` via `tensor_profile_clear_log`.
