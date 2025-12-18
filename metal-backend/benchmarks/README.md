# Benchmarks

Scripts here record hardware details, runtime flags prefixed with `ORCHARD_`, and kernel timings for Tensor v0 operations. Each run writes a commit-scoped JSON file that captures the short Git hash, device information, and per-op microsecond measurements so contributors can track performance regressions over time.

## Usage
Invoke `python benchmarks/run.py -o /tmp/bench` after building to emit a JSON file under `/tmp/bench/<commit>/benchmarks.json` where `<commit>` is the short Git hash. The harness exercises add, mul, matmul, reduce_sum, mean, and transpose kernels through the Tensor API and records one entry per operation. Supplying a different output directory allows side-by-side comparisons between commits.

Benchmark outputs are untracked; generate them locally as needed and attach relevant snippets to pull requests when discussing performance changes.

## Result Format
Each JSON file includes a top-level dictionary keyed by operation name. Entries record average runtime in microseconds, tensor shapes, data types, and whether the kernel executed on the CPU or an mps device. Hardware metadata such as processor model and memory configuration appears under the `system` key.

## Current Status
- Benchmarks cover add, mul, matmul, reduce_sum, mean, and transpose and note
  whether kernels executed on the CPU or an mps device. When
  `ORCHARD_TENSOR_PROFILE` is set the harness embeds lines from
  `/tmp/orchard_tensor_profile.log`.

## Contributor Protocol
- Consult `../AGENTS.md` for the authoritative repository policy.
- Run `cmake -S . -B build -G Ninja` and `cmake --build build --target check` from the repository root before pushing changes; include any diagnostic output in pull requests.
- When configuring offline, supply `-DFETCHCONTENT_FULLY_DISCONNECTED=ON` so the test suite links against the trimmed `third_party/googletest` tree or a system installation without attempting a download.
- Search the repository with `rg` rather than `ls -R` or `grep -R`.
- Do not commit generated JSON outputs or any file exceeding five megabytes.
- Use `clang-format` for C++ and Objective-C++ sources.
- Keep commits single-purpose with an imperative one-line summary and reference touched files by path and line number in the pull request.
- Work on the default branch only and ensure the worktree is clean after committing.
- When profiling behaviour is exercised, reset the cached flag with
  `tensor_profile_reset` after changing `ORCHARD_TENSOR_PROFILE` and clear
  `/tmp/orchard_tensor_profile.log` via `tensor_profile_clear_log`.
