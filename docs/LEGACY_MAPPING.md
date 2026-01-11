# Legacy Mapping

This index records where every pre-consolidation document landed. Each bullet lists the removed file(s) and the section inside the new handbook that now hosts the canonical content. Use it to answer “where did file X go?” without trawling git history.

## Overview (`docs/overview.md`)
- `docs/roadmap.md`, `docs/progress.md`, `docs/detailed_updates.md`, `docs/system_changes.md`, `docs/changelog.md` → [`Document Map`](overview.md#document-map)
- `docs/architecture/README.md`, `docs/architecture/node.md` → [`Repository Layout`](overview.md#repository-layout-live-tree)

## Architecture (`docs/architecture.md`)
- `docs/consensus.md`, `docs/difficulty.md`, `docs/poh.md`, `docs/dkg.md`, `docs/sharding.md`, `docs/macro_block.md`, `docs/blob_chain.md`, `docs/hashlayout.md`, `docs/genesis_history.md`, `docs/serialization.md` → [`Ledger and Consensus`](architecture.md#ledger-and-consensus)
- `docs/transaction_lifecycle.md`, `docs/mempool.md`, `docs/mempool_qos.md`, `docs/account_abstraction.md`, `docs/identity.md`, `docs/tokens.md` → [`Transaction and Execution Pipeline`](architecture.md#transaction-and-execution-pipeline)
- `docs/gossip.md`, `docs/gossip_chaos.md`, `docs/networking.md`, `docs/net_bootstrap.md`, `docs/net_a_star.md`, `docs/network_partitions.md`, `docs/network_topologies.md`, `docs/p2p_protocol.md`, `docs/network_quic.md`, `docs/quic.md`, `docs/swarm.md` → [`Networking and Propagation`](architecture.md#networking-and-propagation)
- `docs/range_boost.md`, `docs/localnet.md`, `docs/headless.md`, `docs/ai_diagnostics.md` → [`LocalNet and Range Boost`](architecture.md#localnet-and-range-boost) and [`Auxiliary Services`](architecture.md#auxiliary-services)
- `docs/storage.md`, `docs/storage_pipeline.md`, `docs/storage_erasure.md`, `docs/storage_market.md`, `docs/storage/repair.md`, `docs/simple_db.md`, `docs/wal.md`, `docs/snapshots.md`, `docs/state_pruning.md`, `docs/snapshot.md` → [`Storage and State`](architecture.md#storage-and-state)
- `docs/compute_market.md`, `docs/compute_market_courier.md`, `docs/compute_snarks.md`, `docs/scheduler.md`, `docs/htlc_swaps.md` → [`Compute Marketplace`](architecture.md#compute-marketplace)
- `docs/dex.md`, `docs/dex_amm.md` → [`DEX and Trust Lines`](architecture.md#dex-and-trust-lines)
- `docs/bridges.md`, `docs/bridge_security.md` → [`Token Bridges`](architecture.md#token-bridges)
- `docs/wallets.md`, `docs/gateway.md`, `docs/gateway_dns.md`, `docs/mobile_gateway.md`, `docs/mobile_light_client.md`, `docs/light_client.md`, `docs/light_client_stream.md`, `docs/light_client_incentives.md`, `docs/read_receipts.md` → [`Gateway and Client Access`](architecture.md#gateway-and-client-access)
- `docs/telemetry.md`, `docs/metrics.md` → [`Telemetry and Instrumentation`](architecture.md#telemetry-and-instrumentation)
- `docs/service_badge.md`, `docs/le_portal.md`, `docs/probe.md` → [`Auxiliary Services`](architecture.md#auxiliary-services)

## Economics and Governance (`docs/economics_and_governance.md`)
- `docs/economics.md`, `docs/inflation.md`, `docs/tokens.md`, `docs/fee_rebates.md`, `docs/fees.md`, `docs/settlement_switch.md`, `docs/settlement_audit.md` → [`BLOCK Supply`, `Fee Lanes`, and `Settlement`](economics_and_governance.md#block-supply-and-sub-ledgers)
- `docs/treasury.md`, `docs/ledger_invariants.md`, `docs/service_badge.md` → [`Treasury`, `Service Badges`, and `Ledger Invariants`](economics_and_governance.md#treasury-and-disbursements)
- `docs/governance.md`, `docs/governance_params.md`, `docs/governance_release.md`, `docs/governance_rollback.md`, `docs/governance_ui.md`, `docs/commit_reveal.md`, `docs/system_changes.md`, `docs/audit_handbook.md`, `docs/risk_register.md` → [`Proposal Lifecycle`, `Governance Parameters`, and `Risk Controls`](economics_and_governance.md#proposal-lifecycle)

## Operations (`docs/operations.md`)
- `docs/deployment.md`, `docs/deployment_guide.md`, `docs/runbook.md`, `docs/operators/README.md`, `docs/operators/run_a_node.md`, `docs/operators/upgrade.md` → [`Bootstrap`, `Running a Node`, and `Deployment`](operations.md#bootstrap-and-configuration)
- `docs/monitoring.md`, `docs/monitoring/README.md`, `docs/dashboard.md`, `docs/telemetry_ops.md`, `docs/metrics.md`, `docs/operators/telemetry.md` → [`Telemetry Wiring`, `Metrics Aggregator Ops`, and `Monitoring`](operations.md#telemetry-wiring)
- `docs/probe.md`, `docs/operators/incident_playbook.md`, `docs/settlement_audit.md`, `docs/settlement_switch.md` → [`Probe CLI and Diagnostics`](operations.md#probe-cli-and-diagnostics) and [`Incident Response`](operations.md#incident-response)
- `docs/storage/repair.md`, `docs/snapshots.md`, `docs/snapshot.md`, `docs/wal.md` → [`Storage, Snapshots, and WAL Management`](operations.md#storage-snapshots-and-wal-management)
- `docs/range_boost.md`, `docs/localnet.md` → [`Range Boost and LocalNet Operations`](operations.md#range-boost-and-localnet-operations)
- `docs/system_changes.md`, `docs/detailed_updates.md`, `docs/changelog.md` → [`Operator Checklist`](operations.md#operator-checklist)

## Security and Privacy (`docs/security_and_privacy.md`)
- `docs/crypto.md`, `docs/crypto_migration.md`, `docs/remote_signer_security.md`, `docs/threat_model/README.md`, `docs/threat_model/hosting.md`, `docs/threat_model/storage.md` → [`Threat Model`, `Cryptography`, and `Remote Signers`](security_and_privacy.md#threat-model)
- `docs/kyc.md`, `docs/jurisdiction.md`, `docs/jurisdiction_authoring.md`, `docs/privacy_layer.md`, `docs/privacy_compliance.md`, `docs/le_portal.md` → [`KYC, Jurisdiction, and Law-Enforcement`](security_and_privacy.md#kyc-jurisdiction-and-compliance)
- `docs/supply_chain.md`, `docs/repro.md`, `docs/reproducible_builds.md`, `docs/provenance.md`, `docs/release_provenance.md`, `docs/pivot_dependency_strategy.md` → [`Release Provenance and Supply Chain`](security_and_privacy.md#release-provenance-and-supply-chain)
- `docs/bridge_security.md`, `docs/htlc_swaps.md`, `docs/risk_register.md`, `docs/audit_handbook.md` → [`Bridge and Cross-Chain Security` and `Risk Register`](security_and_privacy.md#bridge-and-cross-chain-security)

## Developer Handbook (`docs/developer_handbook.md`)
- `docs/developer_setup.md`, `docs/development.md`, `docs/concurrency.md`, `docs/logging.md`, `docs/debugging.md`, `docs/testing.md`, `docs/performance.md`, `docs/benchmarks.md`, `docs/formal.md`, `docs/simulation_framework.md` → [`Developer Handbook: Environment Setup`, `Coding Standards`, `Testing Strategy`, `Performance and Benchmarks`, and `Formal Methods`](developer_handbook.md#environment-setup)
- `docs/contract_dev.md`, `docs/wasm_contracts.md`, `docs/vm_debugging.md`, `docs/vm.md` → [`Developer Handbook: Contract and VM Development`](developer_handbook.md#contract-and-vm-development)
- `docs/demo.md`, `docs/headless.md`, `docs/explain.md`, `docs/ai_diagnostics.md` → [`Developer Handbook: Python + Headless Tooling` and `Explainability`](developer_handbook.md#python--headless-tooling)
- `docs/pivot_dependency_strategy.md`, `docs/dependency_inventory.md`, `docs/first_party_dependency_audit.md`, `docs/contributing.md` → [`Dependency Policy` and `Contribution Flow`](developer_handbook.md#dependency-policy)

## APIs and Tooling (`docs/apis_and_tooling.md`)
- `docs/rpc.md`, `docs/first_party_rpc_blockers.md` → [`JSON-RPC`](apis_and_tooling.md#json-rpc)
- `docs/gateway.md`, `docs/mobile_gateway.md`, `docs/gateway_dns.md`, `docs/wallets.md` → [`Gateway HTTP and CDN Surfaces`](apis_and_tooling.md#gateway-http-and-cdn-surfaces) and [`DNS and Naming`](apis_and_tooling.md#dns-and-naming)
- `docs/explorer.md`, `docs/service_badge.md`, `docs/telemetry.md` → [`Explorer and Log Indexer`](apis_and_tooling.md#explorer-and-log-indexer) and [`Metrics and Telemetry APIs`](apis_and_tooling.md#metrics-and-telemetry-apis)
