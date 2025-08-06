# Audit Handbook

This document consolidates the management directives for audit and development agents working on **the-block**. It outlines the uncompromising standards expected for environment setup, testing, and adversarial review. These requirements supplement `AGENTS.md` and `Agents-Sup.md`.
An automated audit matrix (`xtask gen-audit-matrix`) maps every requirement ID to its verifying test and CI job. CI runs `xtask check-audit-matrix` to ensure docs and code remain in lockstep.

## 1. Environment Determinism
- All scripts and tests must refuse to run if the active Python interpreter is outside `.venv`.
- Build steps print a machine-readable nonce containing the git commit, timestamp, and dependency hashes.
- Tests start from a deliberately "dirty" state with leftover DB files and temporary garbage to verify correct cleanup.
- Each `Blockchain::new` instance writes to its own temp directory via `unique_path`
  and deletes it after tests, proving isolation.

## 2. Attack Surface Exploration
- Begin by attacking the least documented paths. Introduce corrupted database files, malformed blocks, and replayed transactions.
- For every failure reproduced, document the steps, create a test, then fix the issue and repeat.

## 3. Fuzzing and Concurrency
- Fuzz every public API boundary—including Python bindings—with mutation and concurrency stress.
- Hold locks and terminate threads to confirm no deadlocks or state corruption occurs.

## 4. Cryptographic Checks
- Prove domain separation for all signatures and hashes by inspection and automated search for variable domain tags.
- Exhaustively test nonce handling across reorgs, migrations, and corrupted state.

## 5. Schema Migration Discipline
- Every migration is reversible and idempotent. Round-trip fuzz tests migrate back and forth to confirm hash-equivalent state.
- Canonical snapshots for each schema version are hashed and stored to detect drift.

## 6. Red-Team Playbook
- Maintain scripts under `docs/red_team.md` showing how to reproduce attacks: mempool flooding, malformed blocks, corrupted disk state, and network partitions.
- Crash the node at every state boundary (`SIGKILL` before/after disk flush, during migration, etc.) and verify deterministic recovery.

## 7. CI Gating
- Coverage, mutation testing, proofs, and fuzz statistics must all surface as badges that block merges when below threshold.
- CI artifacts include full logs, DB states, random seeds, and proof diffs for audit replay.

## 8. Documentation Integrity
- Every invariant ID in `docs/ledger_invariants.md` links to its implementation line and commit SHA.
- A script scans for orphaned docs or code and fails CI if any mismatch is found.

## 9. Cultural Imperatives
- Always run builds and tests from a fresh environment; no untracked state is allowed.
- When the system appears stable, deliberately break it again to ensure robustness.

**Final Word:** Proof, not hope. The repository is only ready when every path has been forced, documented, tested, and proven resilient to adversarial abuse.
