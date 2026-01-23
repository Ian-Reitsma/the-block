# AGENTS.md – Tensor Library Manual

This file applies to all code, tests, and documentation under `metal-tensor/`. Refer to `/AGENTS.md` at the repository root for overarching policies.

## Discoverability
Locate policy files by running `find . -name AGENTS.md -print` from the repository root. Current files include:
- `/AGENTS.md`
- `docs/AGENTS.md`
- `metal-tensor/AGENTS.md`
Expand this list and update each file whenever new directories add their own `AGENTS.md`.

## Tensor Library Notes
- Production sources live in `metal/`.
- Tests reside in `tests/`; run the full suite before every commit.
- Library-specific documentation sits under `docs/` within this directory.

## Current Status
- Tensor v0 supports intrusive storage, host and device transfers,
  broadcast-aware safe division, axis-correct sum and mean, device-aware
  transpose backward, and profiling that resets with `tensor_profile_reset`
  and clears logs using `tensor_profile_clear_log`.
