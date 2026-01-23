# Benchmarks

Benchmark and orchestration scripts for the experimental PyTorch path live here. Use them to measure FlashAttention or other prototype kernels on top of PyTorch. The scripts assume PyTorch and its dependencies are available and primarily serve regression comparisons against Tensor v0.

Results are not checked into source control. Each run should note the commit hash, input shapes, and whether dropout was enabled so performance changes can be correlated with code revisions.

## Next Steps
Update or remove these scripts once equivalent benchmarking exists for the Metal-native stack and the experimental path is retired.

## Contributor Protocol
- See `../../AGENTS.md` for repository governance and mandatory build and test steps.
- Run `cmake -S . -B build -G Ninja` and `cmake --build build --target check` from the repository root and attach output to pull requests.
- Supply `-DFETCHCONTENT_FULLY_DISCONNECTED=ON` during configuration when network access is unavailable so tests link against the trimmed `third_party/googletest` tree or a system package.
- Use `rg` instead of `ls -R` or `grep -R` when searching the repository.
- Do not commit generated benchmark results or any large artifacts.
- Keep commits single-purpose with an imperative summary line, and reference modified files by path and line number in the pull request.
- Work only on the default branch and maintain a clean worktree.
- When profiling behaviour is exercised, invoke `tensor_profile_reset` after
  toggling `ORCHARD_TENSOR_PROFILE` and delete stale logs with
  `tensor_profile_clear_log`.
