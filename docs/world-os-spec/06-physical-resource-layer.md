# Physical Resource Layer Specification

## Purpose
Extend the unified BLOCK economic engine to measurable physical resources (energy, bandwidth, hardware). The implementation mirrors existing storage/compute markets and uses the new `crates/energy-market` + `crates/oracle-adapter` scaffolding introduced in Step 2. Everything routes through first-party serialization, telemetry, and governance stacks.

## Physical Resource Layer Architecture
### Resource Types
1. **Energy Credits** — kWh metered via registered smart meters or mock oracle services. Stored as `EnergyCredit` receipts within `crates/energy-market`.
2. **Bandwidth Credits** — Future work building on `gateway` read batching and `range_boost` metrics. Shares the same provider registration flow.
3. **Hardware Credits** — GPU/CPU-hour slices with telemetry exported from compute providers. Pricing + EWMA identical to storage.

### Oracle Integration Pattern
- External signals → `OracleAdapter` (HTTP/WebSocket client living in `crates/oracle-adapter`) → Provider profile (persisted in `energy-market` storage map) → Market settlement (node RPC + ledger transfer).
- All physical resources implement a standardized `Provider` trait (energy crate exports `EnergyProvider` + `ProviderProfile` wrappers). Adapter enforces meter registration before accepting readings.
- Measurement verification uses multi-oracle consensus: readings must include signature + optional quorum attestation. Conflicting readings are quorumed via governance-defined threshold; mismatches trigger slashing (`update_energy_provider_ewma` penalty hooks).
- Each verified reading produces a `MeterReceipt` hashed into `EnergyCredit::meter_reading_hash` for audits and explorer display.

### Market Mechanics
- Pricing: same EWMA smoothing factor (`alpha = 0.3`) used by `storage_market::ReplicaIncentive::record_outcome`, implemented via `update_energy_provider_ewma` in the energy crate.
- Quotas: per-jurisdiction enforcement uses `crates/jurisdiction` APIs (`jurisdiction::limit_for("energy")`). Settlements fail fast if provider exceeds quota.
- Settlement cadence: configurable toggles in governance proposal `UpdateEnergyMarketParams` choose between real-time (`settle_energy_delivery` executes immediately) vs batched (settlement engine sweeps pending receipts each block).
- Disputes: governance receives oracle disputes via `contract-cli gov submit --payload UpdateEnergyMarketParams` specifying slashing rate + timeout. CLI `contract-cli energy disputes` surfaces outstanding cases.

### Meter/Oracle Requirements
- Signed readings include Unix timestamp, provider ID, jurisdiction, and measurement payload. Format defined in `crates/oracle-adapter::MeterReading` implementation (JSON encoded, hashed with BLAKE3).
- Cryptographic attestation via provider’s meter public key registered during `register_energy_provider`.
- Heartbeat protocol: adapters submit “still alive” readings (zero delta) every `oracle_timeout` blocks. Missing heartbeat triggers alert + potential suspension.
- Slashing conditions: false reporting, delayed heartbeats, or tampered signatures. When triggered, `energy_market::EnergyProvider` deposit is reduced similar to storage proofs and the event is logged through `foundation_metrics` counters + `kyc` audit trail.

## Settlement Flow (Energy Vertical)
1. Provider calls `register_energy_provider` via CLI/RPC. Node persists provider entry, meter address, stake, and jurisdiction data.
2. Oracle adapter fetches signed readings and submits them through RPC `energy.submit_reading`, which validates signature + timestamp.
3. Consumers run `contract-cli energy settle` referencing provider ID + kWh consumed. Node verifies latest readings, computes price (`price_per_kwh` × amount), applies jurisdiction fee, transfers BLOCK (95% provider, 5% treasury), emits `EnergyReceipt`.
4. EWMA reputation updated via `update_energy_provider_ewma`. Telemetry counters `energy_providers_count`, `energy_kwh_traded_total`, and histograms `oracle_reading_latency_seconds` updated for dashboards.
5. Governance monitors `energy_market_health` (see Step 3.7) to trigger parameter tweaks.

## Data & RPC Surfaces
- **RPC** (to be added): `energy.register_provider`, `energy.market_state`, `energy.settle`, `energy.submit_reading`, `energy.providers`, `energy.receipts`.
- **CLI**: `contract-cli energy register|market|settle|submit-reading` (see Step 3.2 instructions).
- **Storage**: sled tree `energy:providers` storing `EnergyProvider` records, `energy:receipts` storing `EnergyReceipt`, `energy:credits` for outstanding balances.
- **Explorer**: extend `provider_stats` table with `energy_capacity_kwh` and `reputation` columns. Quickstart doc `docs/testnet/ENERGY_QUICKSTART.md` explains flows.

## Security & Compliance Hooks
- All oracle submissions log to KYC/Law-enforcement portal with pack ID + jurisdiction string, reusing `node/src/le_portal.rs` and `docs/architecture.md#auxiliary-services` runbooks.
- Governance proposals controlling energy parameters must reference `Release Provenance and Supply Chain` docs per AGENTS.

This spec binds the physical-resource layer to the rest of the stack. Implementation steps are enumerated in `ROADMAP.md` and Step 2 instructions.
