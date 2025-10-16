# Dependency Inventory

_Last refreshed: 2025-10-14._  The workspace `Cargo.lock` no longer references
any crates from crates.io; every dependency in the graph is now first-party.
The final external cluster—the optional `legacy-format` sled importer—has been
replaced with an in-house manifest shim so the lockfile resolves solely to
workspace crates.

| Tier | Crate | Version | Origin | License | Notes |
| --- | --- | --- | --- | --- | --- |
| _none_ | — | — | — | — | The workspace has zero third-party crates. |

## Highlights

- ✅ RPC fuzzing now routes through the first-party `foundation_fuzz`
  harness and `fuzz_dispatch_request`, removing the last reliance on
  test-only RPC internals.
- ✅ `foundation_fuzz::Unstructured` grew native IP address helpers plus unit
  coverage, simplifying network-oriented fuzz targets.
- ✅ The optional sled legacy importer is now implemented in-house; enabling the
  feature consumes a JSON manifest instead of pulling the crates.io `sled`
  stack, so `FIRST_PARTY_ONLY=1` builds cover the entire workspace.
- ✅ Net and gateway fuzz harnesses dropped `libfuzzer-sys`/`arbitrary`
  in favour of the shared modules and now ship smoke tests that exercise
  the in-tree entry points directly.
- ✅ `foundation_serde` and `foundation_qrcode` no longer expose external
  backends; every consumer—including the remote signer CLI—now relies on
  the stubbed first-party implementations.
- 🚧 Keep regenerating this inventory after large dependency refactors so the
  dashboard and summaries remain accurate.
