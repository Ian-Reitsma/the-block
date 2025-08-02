# Agent/Codex Branch Audit Notes

The following notes catalogue gaps, risks, and corrective directives observed across the current branch. Each item is scoped to the existing repository snapshot (commit `20ac136e`). Sections correspond to the original milestone specifications. Where applicable, cited line numbers reference the repository at the same commit.

## 1. Nonce Handling and Pending Balance Tracking
- **Sequential Nonce Enforcement**: `submit_transaction` checks `tx.payload.nonce != sender.nonce + sender.pending_nonce + 1` (src/lib.rs, L427‑L428). This enforces strict sequencing but does not guard against race conditions between concurrent submissions. A thread‑safe mempool should lock the account entry during admission to avoid double reservation.
- **Pending Balance Reservation**: Pending fields (`pending_consumer`, `pending_industrial`, `pending_nonce`) increment on admission and decrement only when a block is mined (src/lib.rs, L454‑L456 & L569‑L575). There is no path to release reservations if a transaction is dropped or replaced; a mempool eviction routine must unwind the reservation atomically.
- **Atomicity Guarantees**: The current implementation manipulates multiple pending fields sequentially. A failure mid‑update (e.g., panic between consumer and industrial adjustments) can leave the account in an inconsistent state. Introduce a single struct update or transactional sled operation to guarantee atomicity.
- **Mempool Admission Race**: Because `mempool_set` is queried before account mutation, two identical transactions arriving concurrently could both pass the `contains` check before the first insert. Convert to a `HashSet` guarded by a `Mutex` or switch to `dashmap` with atomic insertion semantics.
- **Sender Lookup Failure**: `submit_transaction` returns “Sender not found” if account is absent, but there is no API surface to create accounts implicitly. Decide whether zero‑balance accounts should be auto‑created or require explicit provisioning; document accordingly.

## 2. Fee Routing and Overflow Safeguards
- **Fee Decomposition (`decompose`)**:
  - The function clamps `fee > MAX_FEE` and supports selectors {0,1,2}. Selector `2` uses `div_ceil` to split odd fees. However, the lack of a `match` guard for selector `>2` in `submit_transaction` means callers bypass `decompose` and insert invalid selectors directly into stored transactions. Admission should reject `tx.payload.fee_selector > 2` before persisting.
  - `MAX_FEE` is defined as `(1u64<<63)-1`, satisfying the spec, but there is no doc comment linking to CONSENSUS.md as required.
- **Miner Credit Accounting**:
  - Fees are credited directly to the miner inside the per‑transaction loop (src/lib.rs, L602‑L608) instead of being aggregated into `coinbase_consumer/industrial` and applied once. This violates the “single credit point” directive and complicates block replay proofs.
  - No `u128` accumulator is used; summing many near‑`MAX_FEE` entries could overflow `u64` before the clamp. Introduce `u128` accumulators for `total_fee_ct` and `total_fee_it`, check against `MAX_SUPPLY_*`, then convert to `u64` for coinbase output.
- **Block Header Integrity**:
  - The block header lacks a `fee_checksum` field. Spec requires `blake3(acc_ct‖acc_it)` to be stored and validated on receipt. Update `Block` struct, hashing logic, and validation routines accordingly.
- **Admission Error Codes**:
  - `FeeError::Overflow` and `FeeError::InvalidSelector` map to generic `ValueError` strings in Python (src/fee/mod.rs, L31). The API must expose distinct error codes (`ErrFeeOverflow`, `ErrInvalidSelector`) for downstream clients.
- **Overflow Proof Obligation**:
  - While `MAX_FEE` caps sender fees, miner balance increments are unchecked against `MAX_SUPPLY_*`. Use `checked_add` or explicit bound checks before crediting miner balances to guarantee `INV-FEE-02` end‑to‑end.

## 3. Difficulty Field Verification
- **Hash Inclusion**: `calculate_hash` includes `difficulty` (src/lib.rs, L1199‑L1200). This satisfies structural inclusion, but validation only compares `block.difficulty` against the node’s static `self.difficulty` (src/lib.rs, L656‑L658). There is no computation of the expected target per height nor any retargeting infrastructure. Future dynamic difficulty code must derive the expected value and reject mismatches.
- **Replay Attack Surface**: Without validating difficulty against height, a malicious miner could craft a block with an easier difficulty so long as `leading_zero_bits` meets the easier target. Implement `expected_difficulty(height)` and verify equality during `validate_block`.

## 4. In‑Block Nonce Continuity
- **Assembly Ordering**: `mine_block` blindly appends `self.mempool` to the coinbase (src/lib.rs, L493). If external tooling inserts transactions directly into the mempool (bypassing `submit_transaction`), gaps or out‑of‑order nonces may enter the block and be rejected after costly PoW. Miner should sort transactions by `(from_, nonce)` and skip any that break continuity.
- **Validator Tracking**: `validate_block` reconstructs starting nonces by counting transactions per sender (src/lib.rs, L688‑L704). This assumes the validator knows the account’s confirmed nonce prior to block processing. If the block contains multiple transactions for a sender not present in `self.accounts`, `expected.insert(addr, start)` uses a `saturating_sub` on zero, resulting in `u64::MAX` start; the subsequent check will reject legitimate nonce `1`. Ensure unknown senders default to zero without underflow.

## 5. Mempool Transaction De‑duplication
- **HashSet Keying**: `mempool_set` stores `(address, nonce)` pairs and rejects duplicates (src/lib.rs, L423‑L433). This prevents simple replays but lacks TTL management and memory bounds. An adversary can exhaust memory by submitting unique nonces with zero fee. Introduce a bounded LRU or require a minimum fee floor to discourage spam.
- **Transaction Hash Tracking**: `validate_block` uses `seen.insert(tx.id())` to detect duplicates inside a block (src/lib.rs, L716). However, `tx.id()` depends on signature and payload; two distinct transactions with same `(from, nonce)` but different payloads could still be accepted. For mempool, deduplication already prevents identical nonces, but on block import from peers, an attacker could inject a conflicting transaction not present locally. Block validation should ensure `(from, nonce)` uniqueness regardless of `tx.id()`.

## 6. Database Schema Bump & Migration Testing
- **Schema Versioning**: `ChainDisk` carries `schema_version` and `open()` migrates versions <3 to current (src/lib.rs, L207‑L271). Legacy column families are removed after migration. However:
  - The migration path zeroes `coinbase_*` for legacy blocks without recalculating historical fee data, risking supply drift for pre‑fee blocks.
  - No migration handles accounts lacking `pending_*` fields; existing entries might miss reservations.
- **Unit Test Coverage**: `test_schema_upgrade_compatibility` is annotated with `#[ignore]` (tests/test_chain.rs, L450). CI does not exercise migration paths. Enable the test and add fixtures for v1/v2 layouts, asserting defaulted `pending_*` fields and nonces.
- **Snapshot Integrity**: There is no automated “migrate to v3, roll back 100 blocks, re‑dump” verification as required. Implement a snapshot test ensuring round‑trip hash stability.

## 7. Demo Script Verbosity & Clarity
- The demo narrates basic steps but lacks the requested analogies (“nonce is like a check number”) and detailed explanations of pending balance locking. Expand `explain(...)` calls to cover fee splitting, nonce semantics, and ledger state transitions in plain language.
- Script only covers a single fee selector (`0`); extend to demonstrate IT and split fees to validate cross‑token behaviour.
- No demonstration of error cases (e.g., submitting a transaction with stale nonce). Including intentional failures would clarify validation rules for new developers.

## 8. Documentation Refresh
- **AGENTS.md**: The disclaimer still asserts “educational purposes only,” contradicting the project’s professional positioning. Update to reflect production‑grade intent and relocate cautionary language to README’s disclaimer section.
- **CHANGELOG.md**: Records schema v3 and fee routing, but lacks migration guidance or references to governance artefacts.
- **CONSENSUS.md & ECONOMICS.md**: While sections were appended, inter‑document links are shallow. Use explicit anchors and cross‑references so changes in one doc propagate without duplication.
- **AGENTS Sup (Agents-Sup.md)**: Mentions schema version 3 but lacks detailed instructions for future schema migrations and does not reference the new invariants.

## 9. Invariant Specification (ECONOMICS.md)
- INV‑FEE‑01 and INV‑FEE‑02 are documented with prose and minimal algebra. For formal verification, expand the algebraic chain showing `fee_ct + fee_it = f` and the bounds proofs for each selector case.
- `$comment` in `spec/fee_v2.schema.json` references ECONOMICS.md lines 11‑20 and 22‑30. These line numbers will drift; replace with named anchors or commit hashes to maintain traceability.
- Provide explicit quantification over blocks and transactions in the invariants to reduce ambiguity for F★ translators.

## 10. Admission Pipeline Hardening
- `submit_transaction` conflates all admission errors into generic `ValueError` without machine‑readable codes. Define distinct error enums (`InsufficientCT`, `InsufficientIT`, `FeeOverflow`, `InvalidSelector`) and surface them through PyO3 to keep API stability.
- The function does not log the reservation event despite the spec (“fee_lock: sender=..., ct=.., it=..”). Integrate structured logging for observability.
- Balance checks call `saturating_sub(sender.pending_consumer)` which silently wraps on underflow; replace with explicit comparison to avoid masking bugs.
- The admission path does not verify that amounts plus fees stay below `MAX_SUPPLY_*`, risking overflow during mining even if individual balances are capped.

## 11. Miner‑Side Accounting Directive
- Mining uses `reward_c` and `reward_i` from decayed block rewards but does not incorporate accumulated fees into coinbase amounts. The miner’s account receives fees immediately yet `coinbase_*` fields reflect only rewards. Adjust coinbase transaction to add `total_fee_*` and set header fields accordingly before PoW.
- There is no reset of fee accumulators between blocks; a restart mid‑block could leak partial sums. Use local variables scoped per mining invocation.
- No prevention of per‑transaction partial state application: if a panic occurs during the `for tx in &txs` loop after debiting sender but before crediting recipient, ledger diverges. Batch state changes and commit after all checks succeed.

## 12. Block Validation Directive
- Validator recomputes hash and nonce order but does **not** recompute `total_fee_*` to cross‑check with coinbase fields or a `fee_checksum`. Implement iteration over `block.transactions[1..]` using `fee::decompose` and compare against header totals.
- The validation routine mixes stateful checks (signature verification) with nonce ordering. For improved determinism, perform all stateless checks first, then apply state diffs in a copy‑on‑write ledger to ensure failed blocks leave no residue.
- `import_chain` accepts a chain vector without verifying `fee_checksum` or `expected_nonce` continuity beyond simple checks, enabling a crafted chain to slip through if `validate_block` is not called separately. Integrate validation inside `import_chain` per block.

## 13. Edge‑Case Table & Testing
- ECONOMICS.md includes a table but lacks linkage to executable tests. Create a data‑driven test that loads each row and asserts the expected ledger deltas, marking which invariant would fail if behaviour diverged.
- `tests/fee.rs` covers only basic selectors and overflow; extend to loop over boundary cases (`0..10` and `MAX_FEE`) as required.
- Python tests (`tests/test_fee.py`) mirror Rust unit tests but do not assert invariant preservation or miner/sender balances. Augment tests to construct full transactions and verify supply neutrality.

## 14. Cross‑Language Determinism & Fuzzing
- `tests/fee_vectors.rs` and `tests/test_fee_vectors.py` rely on a static CSV (1000 rows). There is no RNG seed policy or 10 000‑pair property test. Introduce a reproducible PRNG (e.g., ChaCha20 with fixed seed) to generate vectors at build time.
- No PyO3 interoperability test ensuring Python and Rust `decompose` produce identical results across random inputs beyond the CSV. Add tests invoking Rust from Python via PyO3 directly.
- Fuzzing harnesses for `admission_check`, `apply_fee`, and `validate_block` are absent. Extend cargo‑fuzz harness to target these functions with sanitizer flags; run nightly on both `x86_64-unknown-linux-musl` and `aarch64-apple-darwin`.

## 15. Migration Path & Fork Activation
- There is no `governance/` directory or `FORK-FEE-01.json` artefact. Fork logic, feature bits, and handshake negotiation are unimplemented.
- Nodes do not advertise readiness via feature bits; P2P layer is absent, so activation cannot be coordinated. Plan network handshake structure in advance even if networking is pending.
- Snapshot generation prior to fork is undocumented. Provide scripts and CI jobs to produce auditable pre‑fork snapshots including historical fee imbalance annotations.

## 16. CI & Quality Gates
- `.github/workflows` lacks dedicated `fee-unit-tests` or `fee-fuzz-san` jobs. Add workflows enforcing ≥95% coverage on fee logic and nightly fuzz runs capped at 30 min wall time.
- CONTRIBUTING.md does not mention new CI gates or coverage requirements. Update to set expectations for external contributors.
- No schema-lint job verifying `spec/fee_v2.schema.json`. Integrate a JSON schema validator (e.g., `ajv`) and publish the schema artefact on CI.

## 17. Observability & Incident Response
- There are no Grafana dashboards or metrics for fee distribution, reject rates, or overflow incidents. Instrument mining and admission paths with Prometheus counters and provide Grafana configuration examples.
- The repository lacks an “Executive Risk Memo” summarizing past fee vulnerabilities and residual risks. Draft a one-page document under `docs/` and link from `CHANGELOG.md`.
- No run-book entry or environment variable toggle (`BLOCK_DISABLE_FEE_V2`) exists to disable fee logic in emergencies. Implement a runtime flag and document hotfix procedures.

## 18. Miscellaneous Gaps
- **PyO3 Module Structure**: `fee::decompose_py` is exported directly from `lib.rs` without a separate `pyo3_fee.rs` or `__init__.py` mirror, diverging from the spec’s modularization guideline.
- **Canonical Serialization**: `spec/fee_v2.schema.json` lacks examples of valid/invalid objects. Provide `examples` array for quick validation by tooling.
- **Edge‑Case Dust Attack**: No mitigation for mempool flooding with `ν=2, f=1` (dust) beyond admission checks. Implement minimum fee thresholds or dynamic mempool pricing.
- **License Notice**: README license section deviates from standard Apache 2.0 text and may conflict with the repo’s license file (if any). Ensure consistency and compatibility with open‑source tooling.

---

These notes should guide subsequent contributors in elevating the branch to the project’s 0.01 % engineering standard. Each item is a concrete, actionable task to close the gap between current implementation and the specified roadmap.

## 19. Commit History Review Highlights
- The last commit removed leftover badge artifacts, but a systematic audit should validate no SVG or badge workflows remain in submodules or documentation.
- Merge commits (`Fee Routing v2` and `Full-Lifecycle Hardening`) group broad changes; future work should split features into smaller, auditable commits to simplify bisecting and review.
- Early history contains large binary blobs (`pixi.lock` with thousands of lines). A repository rewrite to purge these from git history would reduce clone time and improve auditability.

## 20. Repository Hygiene
- `analysis.txt` and other scratch files live at repo root; convert such documents into tracked design notes under `docs/` or remove them to avoid confusion.
- Ensure every script in `scripts/` has `set -euo pipefail` and consistent shebangs; current `scripts/run_all_tests.sh` lacks error checking for missing tools.
- Add `.editorconfig` to enforce consistent indentation (spaces vs tabs) across Rust, Python, and Markdown sources.

## 21. Testing Infrastructure Gaps
- `tests/test_interop.py` imports `the_block` but lacks assertions around fee decomposition or nonce tracking; extend to cover new consensus rules.
- No integration test spans multiple blocks with split fees (`ν=2`) to prove `INV-FEE-01` over several rounds. Create a randomized ledger test mining ≥100 blocks with mixed selectors.
- Property tests do not seed randomness deterministically (`proptest` with `test_runner.config()`); add explicit seeds so failures reproduce reliably in CI.

## 22. Security Considerations
- Absence of rate limiting on `submit_transaction` exposes the node to CPU-bound DoS. Implement token-bucket or fee-based rate limiting.
- Signature verification uses `ed25519-dalek` but does not enforce batch verification or prehashing; consider `ed25519-zebra` for constant-time operations and add tests for signature malleability regression.
- The `chain_id_py()` function returns a constant with no network isolation. Until P2P is implemented, document the risk of cross-environment replays and consider namespacing test networks via configuration.

## 23. Future-Proofing and Design Debt
- `TokenAmount` uses `u64`; migrating to `u128` later may break serialized formats despite wrapper. Define big-endian encoding or explicit versioning to ease transition.
- Difficulty retargeting (mid-term milestone) will require storing per-block timestamps. Current `Block` struct lacks a timestamp field; adding it now simplifies future upgrades.
- No abstraction over persistence layer yet; start by defining a `Storage` trait to decouple sled usage, aligning with mid-term goal of storage swaps.

## 24. Documentation Cross-References
- `docs/detailed_updates.md` is referenced in code comments (e.g., `calculate_hash`), but file does not exist in repo. Either add the document or update comments to point to existing specs.
- Glossary terms defined in ECONOMICS.md should be backlinked from AGENTS.md §14 to maintain a single authoritative glossary.
- CHANGELOG entries for fee routing lack PR numbers and commit hashes; include them for traceability.

## 25. Build & Release Process
- No release automation or tagging strategy is defined. Introduce `cargo-release` or similar tooling with pre-tag lint/test hooks.
- Wheel builds rely on manual `maturin develop`. Add a `Makefile` or `justfile` encapsulating build/test steps to avoid command drift across agents.
- Ensure artifacts (`fee_v2.schema.json`, audit reports) are published as GitHub release assets for reproducibility.

## 26. External Dependencies
- `Cargo.toml` pins versions but lacks `cargo deny` configuration to track license and security advisories. Set up `cargo deny` with a denylist/allowlist policy.
- `requirements.txt` includes `maturin` and `pytest` but not exact hashes or versions; use `pip-tools` or `uv` to generate a lock file with hashes to prevent supply-chain drift.
- Node tooling is optional but `package.json` is empty; remove it or populate with meaningful scripts to avoid confusion.

## 27. Developer Experience
- Pre-commit hooks enforce venv activation but do not run `cargo fmt` or `ruff` automatically. Integrate these checks to catch style issues before commit.
- Provide a `devcontainer.json` for VS Code users to standardize environment setup, reducing onboarding friction.
- Add a `CONTRIBUTING.md` section on how to run fuzz tests and migrate databases locally to encourage thorough review by external contributors.

## 28. Future Work Tracking
- Create GitHub issues or a roadmap markdown referencing each audit item to assign ownership and track progress. Embed links from this document to the issues once created.
- Set up milestones corresponding to the long-term vision (P2P networking, storage abstraction, governance) to visualize dependency chains.
- Establish a recurring “audit sync” meeting or asynchronous report so contributors regularly update status against this checklist.

---

## 29. Networking Readiness (Mid-Term Milestone)
- No `p2p` module or dependency exists; begin by selecting a networking crate (`libp2p` or `quic`). Draft message schemas for `TxBroadcast`, `BlockAnnounce`, and `ChainRequest`.
- Design peer handshake including feature bits (`0x0004` for FEE_ROUTING_V2`) and schema version negotiation. Stub out structs so future integration does not disturb current consensus code.
- Plan for mempool synchronization: implement gossip with inventory (`inv`) and getdata style flows to prevent duplicate downloads and enable relay suppression.

## 30. Formal Verification Scaffold
- Repository lacks `formal/` directory promised for F★ specs. Create `formal/fee_v2.fst` stubs mirroring the algebra in ECONOMICS.md with type definitions for `FeeSelector`, `FeeDecomp`, and lemmas (`fee_split_sum`, `inv_fee_01`).
- Provide build tooling (`Makefile` or `fstar.mk`) so CI can check F★ files for syntax and type errors even before proofs are completed.
- Document how invariants map to code modules to guide the formal methods team; e.g., `fee::decompose` ↔ `Fstar.Fee.decompose`.

## 31. Summary of Missing Deliverables
- Governance artefacts (`governance/FORK-FEE-01.json`) — absent.
- P2P feature-bit handshake — absent.
- CI jobs (`fee-unit-tests`, `fee-fuzz-san`, `schema-lint`) — absent.
- Migration test (`test_schema_upgrade_compatibility`) — ignored.
- Runtime overflow guards for miner credit — incomplete.
- Documentation cross-links and disclaimer updates — incomplete.
- Fuzz harnesses for `admission_check`, `apply_fee`, `validate_block` — missing.
- Grafana dashboards and risk memo — missing.

---

## 32. Risk Register and Stakeholder Assignments
- Establish a `docs/risk_register.md` tracking each economic and technical risk identified here, owner assignment, mitigation status, and review date.
- Assign Lead Economist to validate invariants and fee algebra; Security Chair to sign off on overflow and nonce logic; QA Lead to monitor fuzz dashboards and CI.
- Schedule pre-fork sign-off meeting and record minutes to satisfy governance and audit requirements.

## 33. Concluding Directive
Every item above is a blocker for a production-grade release. Treat this document as a living specification: update it whenever an issue is resolved, add commit references, and ensure future contributors can trace every consensus change to a documented rationale.

