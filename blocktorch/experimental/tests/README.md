# Tests

This directory collects unit and integration tests for the experimental
PyTorch-based path. Tests rely on PyTorch and its dependencies and do not cover
the new Metal tensor stack.

## Running
Execute the tests with `pytest` from this directory. Ensure PyTorch and all required Python packages are installed.

## Next Steps
Expand or retire these tests as the Metal-native stack reaches parity and the experimental path is removed. Recent additions verify gradients for FlashAttention with dropout enabled, providing coverage while the fused Metal kernels mature.

## Contributor Protocol
- Refer to `../../AGENTS.md` for project-wide procedures on building, testing, and committing.
- Before modifying tests, run `cmake -S . -B build -G Ninja` and `cmake --build build --target check` from the repository root, capturing all output.
- The main suite uses the first-party harness under `metal-tensor/tests/` and requires no external downloads during configuration.
- Use `rg` for repository searches; avoid recursive directory scans with `ls -R` or `grep -R`.
- Do not check in test artifacts or datasets; keep large files under untracked directories.
- Each commit must have a single imperative summary line and pull requests must cite modified files by path and line number.
- Work solely on the default branch and ensure the worktree is clean after committing.
- When profiling behaviour is exercised, invoke `tensor_profile_reset` after
  toggling `ORCHARD_TENSOR_PROFILE` and clear `/tmp/orchard_tensor_profile.log`
  with `tensor_profile_clear_log`.
