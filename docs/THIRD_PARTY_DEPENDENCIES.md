# Non-first-party workspace crates
This repo vendors some third-party crates as workspace members.
This report treats anything *not* listed in `config/first_party_manifest.txt` as third-party/external.

## `ad_market` `0.1.0`
- Manifest: `crates/ad_market/Cargo.toml`
- Directly depended on by:
  - `explorer` (dev)
  - `the_block` (normal)

## `ai_diag` `0.1.0`
- Manifest: `tools/ai_diag/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `auto-profile` `0.1.0`
- Manifest: `tools/auto-profile/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `bench-harness` `0.1.0`
- Manifest: `tools/bench-harness/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `bridge-types` `0.1.0`
- Manifest: `crates/bridge-types/Cargo.toml`
- Directly depended on by:
  - `bridges` (normal)
  - `governance` (normal)
  - `the_block` (normal)

## `contract-cli` `0.1.0`
- Manifest: `cli/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `dependency_registry` `0.1.0`
- Manifest: `tools/dependency_registry/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `dual-key-migrate` `0.1.0`
- Manifest: `tools/dual-key-migrate/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `energy-market` `0.1.0`
- Manifest: `crates/energy-market/Cargo.toml`
- Directly depended on by:
  - `explorer` (normal)
  - `mock-energy-oracle` (normal)
  - `oracle-adapter` (normal)
  - `the_block` (normal)

## `foundation_bigint` `0.1.0`
- Manifest: `crates/foundation_bigint/Cargo.toml`
- Directly depended on by:
  - `crypto_suite` (normal)
  - `the_block` (normal)

## `foundation_fuzz` `0.1.0`
- Manifest: `crates/foundation_fuzz/Cargo.toml`
- Directly depended on by:
  - `the-block-fuzz` (normal)

## `foundation_object_store` `0.1.0`
- Manifest: `crates/foundation_object_store/Cargo.toml`
- Directly depended on by:
  - `tb-sim` (normal)

## `foundation_serde` `0.1.0`
- Manifest: `crates/foundation_serde/Cargo.toml`
- Directly depended on by:
  - `ad_market` (normal)
  - `bridge-types` (normal)
  - `bridges` (normal)
  - `codec` (normal)
  - `coding` (normal)
  - `concurrency` (normal)
  - `contract-cli` (normal)
  - `crypto_suite` (normal)
  - `dex` (normal)
  - `energy-market` (normal)
  - `explorer` (normal)
  - `foundation_rpc` (normal)
  - `foundation_serialization` (normal)
  - `foundation_telemetry` (normal)
  - `gov-graph` (normal)
  - `governance` (normal)
  - `inflation` (normal)
  - `jurisdiction` (normal)
  - `ledger` (normal)
  - `light-client` (normal)
  - `log_index` (normal)
  - `metrics-aggregator` (normal)
  - `mock-energy-oracle` (normal)
  - `oracle-adapter` (normal)
  - `p2p_overlay` (normal)
  - `storage` (normal)
  - `storage_market` (normal)
  - `tb-sim` (normal)
  - `the_block` (normal)
  - `tls_warning` (normal)
  - `verifier_selection` (normal)
  - `wallet` (normal)
  - `zkp` (normal)

## `foundation_serde_derive` `0.1.0`
- Manifest: `crates/foundation_serde_derive/Cargo.toml`
- Directly depended on by:
  - `foundation_serde` (dev)
  - `foundation_serialization` (normal)

## `gateway-fuzz` `0.0.0`
- Manifest: `gateway/fuzz/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `gov-graph` `0.1.0`
- Manifest: `tools/gov_graph/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `indexer` `0.1.0`
- Manifest: `tools/indexer/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `installer` `0.1.0`
- Manifest: `tools/installer/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `legacy_manifest` `0.1.0`
- Manifest: `tools/legacy_manifest/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `log-indexer-cli` `0.1.0`
- Manifest: `tools/log_indexer_cli/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `log_index` `0.1.0`
- Manifest: `crates/log_index/Cargo.toml`
- Directly depended on by:
  - `contract-cli` (normal)
  - `log-indexer-cli` (normal)
  - `the_block` (normal)

## `metrics-aggregator` `0.1.0`
- Manifest: `metrics-aggregator/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `mock-energy-oracle` `0.1.0`
- Manifest: `services/mock-energy-oracle/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `monitoring-build` `0.1.0`
- Manifest: `monitoring/Cargo.toml`
- Directly depended on by:
  - `metrics-aggregator` (normal)
  - `tb-sim` (normal)
  - `xtask` (normal)

## `net-fuzz` `0.0.0`
- Manifest: `net/fuzz/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `oracle-adapter` `0.1.0`
- Manifest: `crates/oracle-adapter/Cargo.toml`
- Directly depended on by:
  - `mock-energy-oracle` (normal)
  - `the_block` (normal)

## `privacy` `0.1.0`
- Manifest: `privacy/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `provenance-verify` `0.1.0`
- Manifest: `tools/provenance_verify/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `python_bridge` `0.1.0`
- Manifest: `crates/python_bridge/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `python_bridge_macros` `0.1.0`
- Manifest: `crates/python_bridge_macros/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `release_notes` `0.1.0`
- Manifest: `tools/release_notes/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `release_tests` `0.1.0`
- Manifest: `tools/release_tests/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `remote-signer` `0.1.0`
- Manifest: `tools/remote-signer/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `storage_market` `0.1.0`
- Manifest: `storage_market/Cargo.toml`
- Directly depended on by:
  - `contract-cli` (normal)
  - `storage` (dev)
  - `the_block` (normal)

## `storage_migrate` `0.1.0`
- Manifest: `tools/storage_migrate/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `tb-debugger` `0.1.0`
- Manifest: `tools/debugger/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `the-block-fuzz` `0.0.0`
- Manifest: `fuzz/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `transport` `0.1.0`
- Manifest: `crates/transport/Cargo.toml`
- Directly depended on by: *(none; may be root tool or only transitive)*

## `verifier_selection` `0.1.0`
- Manifest: `crates/verifier_selection/Cargo.toml`
- Directly depended on by:
  - `ad_market` (normal)
  - `the_block` (dev)

## `zkp` `0.1.0`
- Manifest: `crates/zkp/Cargo.toml`
- Directly depended on by:
  - `ad_market` (normal)
  - `the_block` (normal)

