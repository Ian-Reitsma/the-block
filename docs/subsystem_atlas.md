# Subsystem Atlas

Quick reference for navigating the codebase. Find what you need fast.

## Common Tasks → Paths

When you want to do something specific, here's where to look:

| Task | Code Paths | Doc References |
|------|------------|----------------|
| **Change how transaction fees work** | `node/src/fee`, `governance/src/params.rs`, `cli/src/fee_estimator.rs` | [`economics_and_governance.md#fee-lanes-and-rebates`](economics_and_governance.md#fee-lanes-and-rebates) |
| **Add a new governance parameter** | `governance/src/params.rs`, `node/src/governance/params.rs`, `cli/src/gov.rs` | [`economics_and_governance.md#governance-parameters`](economics_and_governance.md#governance-parameters) |
| **Integrate a new data source into the ad market** | `crates/ad_market`, `node/src/ad_policy_snapshot.rs`, `node/src/ad_readiness.rs` | [`architecture.md#ad-market`](architecture.md) |
| **Add a new RPC endpoint** | `node/src/rpc/*.rs`, `cli/src/*.rs` (corresponding CLI wrapper) | [`apis_and_tooling.md#json-rpc`](apis_and_tooling.md#json-rpc) |
| **Modify energy market behavior** | `crates/energy-market`, `node/src/energy.rs`, `node/src/rpc/energy.rs`, `cli/src/energy.rs` | [`architecture.md#energy-market`](architecture.md#energy-market) |
| **Change compute market pricing/SLA** | `node/src/compute_market/settlement.rs`, `governance/src/params.rs` | [`architecture.md#compute-marketplace`](architecture.md#compute-marketplace) |
| **Add telemetry for a new metric** | `node/src/telemetry.rs`, update dashboards in `monitoring/` | [`architecture.md#telemetry-and-instrumentation`](architecture.md#telemetry-and-instrumentation) |
| **Modify consensus rules** | `node/src/consensus`, `node/src/blockchain` | [`architecture.md#ledger-and-consensus`](architecture.md#ledger-and-consensus) |
| **Update storage pipeline** | `node/src/storage/pipeline.rs`, `coding/`, `storage_market/` | [`architecture.md#storage-and-state`](architecture.md#storage-and-state) |
| **Add a new CLI command** | `cli/src/*.rs`, register in `cli/src/main.rs` | [`apis_and_tooling.md#cli`](apis_and_tooling.md#cli-tb-cli) |
| **Configure launch governor gates** | `node/src/launch_governor/mod.rs`, `node/src/governor_snapshot.rs`, `node/src/rpc/governor.rs` | [`architecture.md#launch-governor`](architecture.md#launch-governor), [`operations.md#launch-governor-operations`](operations.md#launch-governor-operations) |

## Subsystem Directory

### Core Infrastructure

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `node/` | Full node implementation | `src/consensus`, `src/blockchain`, `src/mempool`, `src/rpc` |
| `crates/` | Shared libraries | `foundation_*`, `transport`, `httpd`, `storage_engine`, `p2p_overlay`, `wallet` |
| `cli/` | Command-line interface | `src/main.rs`, `src/gov.rs`, `src/wallet.rs`, `src/energy.rs` |
| `governance/` | Governance logic | `src/params.rs`, `src/treasury.rs`, `src/bicameral.rs`, `src/proposals.rs` |

### Markets

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `crates/energy-market/` | Energy trading marketplace | Types, settlement, oracle integration |
| `node/src/compute_market/` | Compute job marketplace | `scheduler.rs`, `settlement.rs`, `snark.rs` |
| `crates/ad_market/` | Privacy-aware ad system | `badge.rs`, `budget.rs`, `privacy.rs`, `uplift.rs` |
| `storage_market/` | Decentralized storage | Rent, providers, redundancy |
| `dex/` | Decentralized exchange | Order books, trust lines, HTLCs |

### Networking & Transport

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `crates/transport/` | QUIC transport abstraction | Quinn, s2n providers |
| `node/src/p2p/` | Peer-to-peer networking | `handshake.rs`, `wire_binary.rs` |
| `node/src/gossip/` | Gossip protocol | `relay.rs` |
| `node/src/localnet/` | Device-to-device mesh | Proximity proofs |
| `node/src/range_boost/` | Extended coverage | Queue, forwarder |

### Gateway & Clients

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `gateway/` | HTTP gateway stack | Mobile cache, DNS publishing |
| `node/src/gateway/` | Gateway integration | `http.rs`, `dns.rs`, `mobile_cache.rs`, `read_receipt.rs` |
| `node/src/light_client/` | Lightweight sync | Header streaming, proofs |

### Infrastructure

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `bridges/` | Cross-chain bridges | `light_client.rs`, `relayer.rs`, `token_bridge.rs` |
| `metrics-aggregator/` | Metrics collection | Dashboard endpoints |
| `monitoring/` | Grafana/Prometheus | JSON dashboards |
| `explorer/` | Block explorer | Web UI, APIs |

### Security & Identity

| Directory | Purpose | Key Files |
|-----------|---------|-----------|
| `crypto/` | Cryptographic primitives | Hashes, signatures |
| `crates/crypto_suite/` | Crypto abstractions | Ed25519, Dilithium, Kyber |
| `node/src/identity/` | DIDs and handles | `handle_registry.rs` |
| `dkg/` | Distributed key generation | Committee keys |
| `zkp/` | Zero-knowledge proofs | SNARK verification |
| `privacy/` | Privacy utilities | Read receipt privacy |

## Legacy Name Mapping

If you see old names in code, here's what they mean now:

| Old Name | Current Name | Notes |
|----------|--------------|-------|
| `amount_it` | Industrial share of CT | Sub-ledger accounting, not a separate token |
| `payout_it` | Industrial payout | Same as above |
| Various `docs/*.md` that no longer exist | See [`LEGACY_MAPPING.md`](LEGACY_MAPPING.md) | Consolidated into main docs |

See [`LEGACY_MAPPING.md`](LEGACY_MAPPING.md) for the full historical mapping.
