# Project Progress Snapshot

This document tracks high‑fidelity progress across The‑Block's major work streams.  Each subsection lists the current completion estimate, supporting evidence with canonical file or module references, and the remaining gaps.  Percentages are rough, *engineer-reported* gauges meant to guide prioritization rather than marketing claims.

## 1. Consensus & Security — ~80 %

**Evidence**
- Hybrid PoW/PoS chain: `node/src/consensus/pow.rs` embeds PoS checkpoints and `node/src/consensus/fork_choice.rs` prefers finalized chains.
- EIP‑1559 base fee tracker: `node/src/fees.rs` adjusts per block and `node/tests/base_fee.rs` verifies target fullness tracking.
- Adversarial rollback tests in `node/tests/finality_rollback.rs` assert ledger state after competing forks.

**Gaps**
- Formal safety/liveness proofs under `formal/` still stubbed.
- No large‑scale network rollback simulation.

## 2. Networking & Gossip — ~72 %

**Evidence**
- Deterministic gossip with partition tests: `node/tests/net_gossip.rs` and docs in `docs/networking.md`.
- Peer identifier fuzzing prevents malformed IDs from crashing DHT routing (`net/fuzz/peer_id.rs`).
- Manual DHT recovery runbook (`docs/networking.md#dht-recovery`).

**Gaps**
- QUIC transport and large‑scale WAN chaos experiments remain open.
- Bootstrap peer churn analysis missing.

## 3. Credits & Governance — ~76 %

**Evidence**
- Credit issuance proposals surfaced via `node/src/rpc/governance.rs` and web UI (`tools/gov-ui`).
- Push notifications on credit balance changes (`crates/wallet/src/credits.rs`).
- Explorer indexes settlement receipts with query endpoints (`explorer/src/lib.rs`).

**Gaps**
- No on‑chain treasury or proposal dependency system.
- Governance rollback simulation incomplete.

## 4. Storage & Free‑Read Architecture — ~68 %

**Evidence**
- Read receipts and reward pool wiring documented in `docs/storage_pipeline.md`.
- Disk‑full metrics and recovery tests (`node/tests/storage_disk_full.rs`).
- Gateway HTTP parsing fuzz harness (`gateway/fuzz`).

**Gaps**
- Incentive‑backed DHT storage marketplace still conceptual.
- Offline escrow reconciliation absent.

## 5. Smart‑Contract VM — ~45 %

**Evidence**
- Persistent `ContractStore` with CLI deploy/call flows (`state/src/contracts`, `cli/src/main.rs`).
- ABI generation from opcode enum (`node/src/vm/opcodes.rs`).
- State survives restarts (`node/tests/vm.rs::state_persists_across_restarts`).

**Gaps**
- Instruction set remains minimal; no formal VM spec or audits.
- Developer SDK and security tooling pending.

## 6. Compute Marketplace & CBM — ~58 %

**Evidence**
- Deterministic GPU/CPU hash runners (`node/src/compute_market/workloads`).
- Price board persistence with metrics (`docs/compute_market.md`).
- Economic simulator outputs KPIs to CSV (`sim/src`).

**Gaps**
- Heterogeneous hardware scheduling and escrowed payments unsolved.
- SLA enforcement rudimentary.

## 7. Trust Lines & DEX — ~70 %

**Evidence**
- Persistent order books via `node/src/dex/storage.rs` and restart tests (`node/tests/dex_persistence.rs`).
- Cost‑based multi‑hop routing with fallback paths (`node/src/dex/trust_lines.rs`).
- Trade logging and metrics (`docs/dex.md`).

**Gaps**
- On‑ledger settlement proofs and partial payments not implemented.
- Escrow for cross‑chain DEX routes absent.

## 8. Wallets & Light Clients — ~70 %

**Evidence**
- CLI + hardware wallet support (`crates/wallet`).
- Mobile light client with push notification hooks (`examples/mobile`, `docs/mobile_light_client.md`).
- Optional KYC provider wiring (`docs/kyc.md`).

**Gaps**
- Remote signer and multisig flows missing.
- Production‑grade mobile apps not yet shipped.

## 9. Bridges & Cross‑Chain Routing — ~25 %

**Evidence**
- Lock/unlock bridge contract with relayer proofs (`bridges/src/lib.rs`).
- CLI deposit/withdraw flows (`cli/src/main.rs` subcommands).
- Bridge walkthrough in `docs/bridges.md`.

**Gaps**
- Light‑client verification and relayer incentive mechanisms undeveloped.
- No safety audits or circuit proofs.

## 10. Monitoring & Telemetry — ~70 %

**Evidence**
- Prometheus exporter with extensive counters (`node/src/telemetry.rs`).
- Monitoring stack via `make monitor` and docs in `docs/monitoring/README.md`.
- Settlement audit CI job (`.github/workflows/ci.yml`).

**Gaps**
- Bridge and VM metrics are sparse.
- Automated anomaly detection not in place.

## 11. Economic Simulation & Formal Verification — ~46 %

**Evidence**
- Simulation scenarios for inflation/demand/backlog (`sim/src`).
- F* scaffolding for consensus proofs (`formal/` installers and docs).
- Scenario library exports KPIs to CSV.

**Gaps**
- Formal proofs beyond scaffolding missing.
- Scenario coverage still thin.

## 12. Mobile UX & Contribution Metrics — ~55 %

**Evidence**
- Background sync respecting battery/network constraints (`docs/mobile_light_client.md`).
- Contribution metrics and optional KYC in mobile example (`examples/mobile`).
- Push notifications for credit events (`crates/wallet/src/credits.rs`).

**Gaps**
- Broad hardware testing and production app distribution outstanding.
- Remote signer support for mobile not yet built.

---

*Last updated: 2025‑05‑15*
