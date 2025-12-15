# World OS Roadmap

## Phase 1 — Core L1 Audit & Stabilization
- Validate consensus/sharding specs in `01-core-l1.md` against current code (`node/src/consensus`, `node/src/blockchain`).
- Run determinism + replay suites (`make sim`, `cargo nextest -p node --features telemetry`).
- Document storage/compute/treasury wiring (completed via this spec) and ensure metrics match monitoring dashboards.
- Outcome: signed-off baseline enabling physical-resource experiments.

## Phase 2 — Energy Credits Vertical MVP
- Land `crates/energy-market` + `crates/oracle-adapter` (Step 2) with provider registration, oracle fetch/submit, EWMA reputation updates, and settlement plumbing.
- Expose RPC `energy.*` endpoints plus CLI `contract-cli energy` commands. Update dashboards + telemetry counters.
- Build `services/mock-energy-oracle` and smoke-test flows locally. Add docs (`docs/testnet/ENERGY_QUICKSTART.md`).
- Outcome: feature-complete vertical gated behind feature flag or governance parameter.

## Phase 3 — Multi-Resource Testnet
- Ship `node/src/chain_spec_worldos.rs` with `worldos_testnet_config`. Launch public nodes + mock meters.
- Add bandwidth/hardware resource adapters (reuse energy crate patterns). Expand monitoring stack + dashboards.
- Host public feedback loop (GitHub Discussions / Discord). Automate health checks (`check_energy_market_health`).
- Outcome: persistent testnet running energy credits + telemetry instrumentation.

## Phase 4 — Mainnet Preparation
- Harden governance integration (new proposal types, treasury dependency guards) and finalize jurisdiction pack updates.
- Conduct WAN-scale chaos drills covering QUIC failover, oracle outages, and settlement slashing.
- Finalize release provenance (cargo vendor snapshots, `provenance.json`) for new crates. Prepare explorer/CLI docs for external operators.
- Outcome: readiness report for “World OS - Energy” launch, including sign-offs from consensus, governance, storage, and telemetry owners.
