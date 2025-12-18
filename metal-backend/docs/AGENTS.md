# AGENTS.md – Documentation Directory Manual

This file governs all content under `docs/`. Consult `/AGENTS.md` at the repository root for global policies.

## Discoverability
Run `find . -name AGENTS.md -print` from the repository root to list every policy file. At the time of this revision the inventory is:
- `/AGENTS.md`
- `docs/AGENTS.md`
- `metal-tensor/AGENTS.md`
Add new entries and update this list whenever additional directories introduce their own `AGENTS.md`.

## Documentation Guidelines
- Keep narratives concise and reference code by path and line number where relevant.
- When instructions change in the root `AGENTS.md`, mirror applicable guidance here and in any `README.md` files within this directory.

## Current Status
- Tensor v0 handles intrusive storage, host and device transfers, broadcast-aware
  safe division, axis-correct sum and mean, device-aware transpose backward, and
  profiling that resets with `tensor_profile_reset` and clears logs via
  `tensor_profile_clear_log`.
