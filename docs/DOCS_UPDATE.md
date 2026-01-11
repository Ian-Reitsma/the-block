# DOCS_UPDATE

This backlog catalogues every documentation stretch that is currently out of sync with the codebase and/or the README’s “BLOCK = single transferable currency” story. Each entry points to the exact file/line range, explains why it is misleading or obsolete, and lists the code surface (or other docs) that now define the ground truth. Update the referenced docs (and, if needed, the schema/json snippets they embed) before changing any corresponding behaviour so the “spec-first” contract stays intact.

## 1. Naming & terminology drift (BLOCK vs. BLOCK)

### `README.md:25-47`, `README.md:117-134`, `README.md:170-189`
- **What’s wrong**: The README introduces BLOCK as the new fixed-supply currency, but the “quick demo” commands and supporting sentences still talk about “sending BLOCK” (see `tb-cli tx send --help` comment and the “docs/economics...” description on lines 170‑179). The README needs to consistently refer to the currency as BLOCK (the codebase still tracks `amount` internally, but `README` is the “public spec” and the place where fresh contributors learn the contract).
- **Fix**:
  1. Replace `“sending BLOCK”` and similar comments on lines 129‑132 with “sending BLOCK” (including the CLI help comment and any other quick-start samples that mention BLOCK or BLOCK sub-ledger tokens).
  2. Update the “Documentation guide” text so it briefly notes that the canonical token name is BLOCK but the code still uses historical `BLOCK/IT` field names for traceability (reference `governance/src/treasury.rs` and `node/src/treasury_executor.rs` as proof that only one token is ever moved).
  3. Confirm the remainder of the README (the “Key features” table, doc map, etc.) speaks in BLOCK terms; add a sentence referencing `docs/economics_and_governance.md#block-supply-and-sub-ledgers` to explain that “BLOCK” now just means the ledger slot/blockchain unit for BLOCK.
  - **Status**: ✅ Completed this week—README now uses BLOCK in the quick-start sample, documents the legacy BLOCK labels, and points readers to the BLOCK-focused economics doc (the note references `governance/src/treasury.rs`, `node/src/treasury_executor.rs`, and `metrics-aggregator/src/lib.rs` for context).

### `AGENTS.md:251-299`, `AGENTS.md:173-185`, `AGENTS.md:269-299`
-- **What’s wrong**: AGENTS is still describing the network as a “single-token (BLOCK)” system, calls the subsidy engine “BLOCK ledger”, and repeatedly cites `BLOCK Supply`/`Fee Lanes` docs (lines 251‑299). This conflicts with README and the glossary that now emphasize BLOCK; we should highlight that subsidy buckets such as `STORAGE_SUB`, `READ_SUB`, and `COMPUTE_SUB` are the canonical BLOCK-denominated ledgers, and mention that any `_CT` suffix references are historical.
- **Fix**:
  1. Change the Project Mission / Economic Model section (lines 251‑299) to call BLOCK the single supply and mention BLOCK/IT as backward-compatible field names in the code (reference `governance/src/treasury.rs`, `node/src/treasury_executor.rs`, `node/src/governance/codec.rs` for why `amount`/`amount_it` still exist).
  2. Update every reference to the BLOCK subsidy buckets (`STORAGE_SUB`, `READ_SUB`, `COMPUTE_SUB`) to note that they are **BLOCK-denominated subsidy buckets** and ensure the old `_CT` suffixed aliases are described as legacy names.
  3. Adjust the governance/fee-floor change walkthrough (lines 119‑144) so the prose says “BLOCK fee floors” rather than “BLOCK fee floors” and links to the `governance` crate and `node/src/fee` for the live policy knobs.
  - **Status**: ✅ Mission section now emphasises BLOCK, includes a legacy BLOCK label note, and the governance walkthrough plus subsidy bullets mention BLOCK explicitly.

### `docs/overview.md:5-12`, `docs/overview.md:30-53`
- **What’s wrong**: The “If you’re brand new” callout still mixes the canonical BLOCK currency with the legacy “Consumer Token” label (lines 5‑12 and 30‑53). That contradicts the README’s BLOCK messaging and leaves new contributors wondering whether there are two tokens.
- **Fix**:
  1. Reword the top-level concepts to say “BLOCK is the single currency; BLOCK/IT are legacy ledger labels”. The table needs to mention BLOCK everywhere, and you can point to the “legacy mapping” note to explain why BLOCK is still visible in the code (link to `docs/LEGACY_MAPPING.md` and `governance/src/treasury.rs`).
  2. Add a short paragraph under the paragraph starting “The Block is the unification layer...” that references `README.md:25-47` and clarifies that the “BLOCK” terminology still appears in code and telemetry because of historical field names (`governance::store::TreasuryBalances`, `metrics-aggregator` gauges, etc.).

  - **Status**: ✅ Added a BLOCK-focused concept table and a legacy label note pointing at the governance/metrics sources.
### `docs/operations.md:26-27`, `docs/operations.md:166`
- **What’s wrong**: The quick “Testnet/Mainnet” definitions (lines 26‑27) still talk about “fake BLOCK” versus “real BLOCK”, and later sections (line 166) mention “BLOCK amount” when describing treasury dashboards. Since the operational guidance is the reference for deployers, it must speak in BLOCK and explain that BLOCK is only the ledger nomenclature inside the codebase.
- **Fix**:
  1. Replace “fake BLOCK/real BLOCK” with “BLOCK (test versus production)”. Mention that `BLOCK` is the internal field name (`governance`: `amount`, `metrics-aggregator`: `treasury_disbursement_amount`) but operators still think BLOCK.
  2. In the dashboard paragraph, rename `BLOCK amount` to `BLOCK amount` and note that the metrics aggregator emits `treasury_disbursement_amount` as a BLOCK-denominated gauge.
  - **Status**: ✅ Updated the testnet/mainnet explainer and treasury dashboard note so both call out BLOCK with a pointer to `treasury_disbursement_amount`.

### `docs/security_and_privacy.md:11`
- **What’s wrong**: The threat-model table starts by saying defenders are protecting “BLOCK” (line 11). This conflicts with README and README‑level messaging and creates unnecessary friction when security reviewers search for BLOCK. 
- **Fix**: Reword the threat-model intro to say “BLOCK” (e.g., `Token thieves | Steal BLOCK...`). Add a parenthetical that `BLOCK` is the field name in the ledger code (`governance/src/store.rs`, `governance/src/treasury.rs`). That ensures the security narrative aligned with the canonical token name while still acknowledging the code terms.
  - **Status**: ✅ Reworded the threat-model entry so it mentions BLOCK with a nod to the BLOCK field labels in `governance/src/store.rs`.

## 2. Stale references / missing source files

### `docs/developer_handbook.md:82-128`
- **What’s wrong**: This chapter still references a laundry list of docs that no longer exist (`docs/benchmarks.md`, `docs/contract_dev.md`, `docs/wasm_contracts.md`, `docs/vm_debugging.md`, `docs/headless.md`, `docs/explain.md`, `docs/ai_diagnostics.md`, `docs/logging.md`, `docs/pivot_dependency_strategy.md`). The current text implies the user should go look at those files, but they have been merged into this handbook or relocated. That hurts discoverability and causes MDBook to emit 404s if someone follows the links.
- **Fix**:
  1. Replace the “Benchmark” bullet (line 82) with a concise pointer such as: “Benchmark thresholds live under `config/benchmarks/<name>.thresholds`, the metrics exporter compares `monitoring/metrics.json`, and Grafana’s `Benchmarks` row visualizes the same thresholds.” Remove the dead `docs/benchmarks.md` link.
  2. Remove every mention of `docs/contract_dev.md`, `docs/wasm_contracts.md`, `docs/vm_debugging.md`, and the other missing files (lines 86‑99, 101‑104, 126‑129) and, if necessary, replace them with existing subsections inside this same handbook or with references to `docs/architecture.md` and `cli/src/<cmd>`. For example, mention “WASM tooling details now live in `docs/architecture.md#virtual-machine-and-wasm` and `node/src/vm`” and delete the `docs/contract_dev.md` link.
  3. Replace the “demo/headless/explain/AI” bullet (lines 101‑129) with the actual content that now lives in this handbook: mention `demo.py`, `cli/src/headless.rs`, `tb-cli explain ...`, and the `ai_diagnostics_enabled` knob, but remove the references to missing Markdown files.
  4. Rewrite the dependency-policy bullet (around line 108) so it no longer points to `docs/pivot_dependency_strategy.md`. Instead reference `config/dependency_policies.toml` plus `AGENTS.md §0.6` and `docs/security_and_privacy.md#release-provenance-and-supply-chain` (those files now capture the “pivot strategy” described here).
  - **Status**: ✅ Rebuilt the handbook section so it no longer references deleted docs and describes benchmarks, contract tooling, Python/headless, dependency policy, and logging inline.
  5. Remove the reference to `docs/logging.md` (line 122). If “logging guidelines” now live in `docs/security_and_privacy.md` or this handbook’s “Logging and Traceability” section, update the text to point there directly.

### `docs/apis_and_tooling.md:142-230`
- **What’s wrong**: The “Treasury disbursement CLI, RPC, and schema” chunk appears verbatim twice (lines 142‑185 and again at 187‑230). The duplicate JSON schema blocks, RPC call list, and CLI commands create needless churn whenever this section gets edited and risk drifting out of sync. Readers will also be confused about whether the second block contains new content.
- **Fix**:
  1. Remove the repeated copy (the second `### Treasury disbursement CLI, RPC, and schema` header at line 187 and everything beneath it through line 230). Keep a single, canonical section that includes the JSON schema, RPC list, and CLI explanations.
  2. While merging, double-check that the `DisbursementPayload` example and schema still match the current `governance` crate (`governance/src/codec.rs` and `examples/governance/disbursement_example.json`). If the schema now includes other fields (e.g., `expected_receipts` entries with BLOCK denominations), make sure the doc describes them accurately and emphasises that `amount_it` is an industrial sub-ledger share rather than a separate token.
  - **Status**: ✅ Deduped the treasury section and now rely on the single remaining block that mirrors `examples/governance/disbursement_example.json`.
  3. After deduping, confirm there’s only one mention of the `Explorer’s REST API mirrors the RPC fields...` paragraph so the doc doesn’t repeat the same guidance twice.

### Missing doc references (guide to what’s been deleted)
- `docs/demo.md`, `docs/headless.md`, `docs/explain.md`, `docs/ai_diagnostics.md`, `docs/contract_dev.md`, `docs/wasm_contracts.md`, `docs/vm_debugging.md`, `docs/vm.md`, `docs/benchmarks.md`, `docs/logging.md`, and `docs/pivot_dependency_strategy.md` are still referenced from `docs/LEGACY_MAPPING.md`/`docs/developer_handbook.md` but no longer exist. Each of those references must:
  1. Either point to the section within `docs/developer_handbook.md` that now covers the same material (e.g., the “Energy Market Development” subsection absorbs the old energy docs) or to the new canonical doc (e.g., `docs/security_and_privacy.md#release-provenance` for dependency pivots).
  2. If we still want easily searchable text for “Explainability,” create a short sub-section inside `docs/developer_handbook.md` or another surviving doc and link to that instead of the deleted file.
  - **Status**: ✅ Legacy references now point to `docs/developer_handbook.md#python--headless-tooling`, `#contract-and-vm-development`, and related anchors, so the map stays accurate.

## 3. Additional cleanup

### `docs/LEGACY_MAPPING.md`
- **What’s wrong**: The legacy map still lists the old `.md` files even though they were merged (see the exact list under “Environment Setup” and “Python + Headless Tooling”). As soon as the handbook is updated, refresh this file to drop the dead references or to link to the consolidated sections inside `docs/developer_handbook.md`.
- **Fix**: After we correct `docs/developer_handbook.md`, update the corresponding rows in `docs/LEGACY_MAPPING.md`/`docs/book/LEGACY_MAPPING.html` so they point to the new sections (e.g., replace the “docs/demo.md” entry with `docs/developer_handbook.md#python--headless-tooling`).
  - **Status**: ✅ Legacy-map rows now link to the updated handbook anchors so the map stays accurate.

### `docs/system_reference.md` (optional future work)
- **Note**: The system reference still describes metrics in `BLOCK` units (see the metric table around the middle of the file). Once the naming of BLOCK → BLOCK is settled, sweep this reference (and the metric names) as part of the rename effort so the system reference reflects the same terminology as the rest of the docs.

---

## Next steps
1. Align the README/overview/AGENTS/operations/security docs so they all refer to BLOCK as the canonical token and describe BLOCK/IT as legacy ledger fields. 
2. Remove the duplicate treasury section from `docs/apis_and_tooling.md` and reconcile the schema snippet with `governance/src/codec.rs`.
3. Replace the dead `docs/*.md` references in `docs/developer_handbook.md` with pointers to existing content (`config/benchmarks`, `docs/architecture`, `docs/security_and_privacy`, etc.) and keep `docs/LEGACY_MAPPING.md` in sync.
4. Re-run `mdbook build docs` and skim the output for broken links after every change; we should not ship doc edits with unresolved references.
