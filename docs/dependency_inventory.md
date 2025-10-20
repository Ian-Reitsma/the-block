# Dependency Inventory

_Last refreshed: 2025-10-19._  The workspace `Cargo.lock` no longer references
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
- ✅ Ledger persistence and startup rebuild now consume the cursor-backed
  `ledger_binary` helpers end to end: `MempoolEntryDisk` stores a cached
  `serialized_size`, the rebuild path uses it before re-encoding, and new unit
  tests cover `decode_block_vec`, `decode_account_map_bytes`, and
  `decode_emission_tuple` so no `binary_codec` fallbacks remain for legacy
  snapshots.
- ✅ The node RPC client now emits JSON-RPC envelopes through manual
  `foundation_serialization::json::Value` builders and decodes responses without
  invoking `foundation_serde` derives, preventing the stub backend from firing
  during `mempool`/`stake`/`inflation` client calls.
- ✅ Storage provider-profile compatibility tests now rely on the cursor writer
  that production code uses, dropping the last `binary_codec::serialize`
  invocation from the suite while preserving randomized EWMA/throughput checks.
- ✅ Gossip peer telemetry tests and the aggregator failover harness assert
  against the shared `peer_snapshot_to_value` helper, keeping networking JSON
  construction entirely first party during CI runs.
- ✅ `foundation_fuzz::Unstructured` grew native IP address helpers plus unit
  coverage, simplifying network-oriented fuzz targets.
- ✅ The optional sled legacy importer is now implemented in-house; enabling the
  feature consumes a JSON manifest instead of pulling the crates.io `sled`
  stack, so `FIRST_PARTY_ONLY=1` builds cover the entire workspace.
- ✅ Gossip messages, ledger blocks, and transactions now encode via
  `net::message`, `transaction::binary`, and `block_binary` cursor helpers,
  removing the remaining `binary_codec` shim usage while new tests lock payload
  order and legacy parity across handshake/drop maps and DEX/storage manifests.
- ✅ Net and gateway fuzz harnesses dropped `libfuzzer-sys`/`arbitrary`
  in favour of the shared modules and now ship smoke tests that exercise
  the in-tree entry points directly.
- ✅ `foundation_serde` and `foundation_qrcode` no longer expose external
  backends; every consumer—including the remote signer CLI—now relies on
  the stubbed first-party implementations.
- 🚧 Keep regenerating this inventory after large dependency refactors so the
  dashboard and summaries remain accurate.
