# Jurisdiction Packs & Compliance Surface

## 1. Modules & Data Sources
- `crates/jurisdiction/` — Defines `JurisdictionId`, policy packs, fee overrides, feature toggles, and audit logging helpers. Policy manifests live under `crates/jurisdiction/policies/*.toml`.
- `node/src/jurisdiction.rs` & `node/src/kyc.rs` — Apply policy packs to gateway, node CLI, and RPC flows. Jurisdiction modules feed law-enforcement logging per AGENTS spec.
- `node/src/rpc/jurisdiction.rs` — JSON-RPC endpoints `jurisdiction.active_pack`, `jurisdiction.list`, `jurisdiction.override`, `jurisdiction.reset`. CLI `contract-cli jurisdiction ...` references these methods.
- `docs/security_and_privacy.md#kyc-jurisdiction-and-compliance` — Canonical narrative for compliance posture.

## 2. State & Storage Layout
| Item | Path | Description |
| --- | --- | --- |
| `jurisdiction::Pack` | `crates/jurisdiction/src/lib.rs` | Contains pack ID, region code, policy version, read/write quotas, feature toggles, fee modifiers, LE logging flags. Serialized via `foundation_serialization`. |
| `jurisdiction::Store` | same | sled tree (default `jurisdiction.db/`) keyed by `pack:<id>` storing pack definitions + override state. |
| `node/src/kyc.rs::AuditEntry` | Records LE queries, jurisdiction overrides, and portal usage. Stored in `kyc:audit`. |

## 3. RPC/CLI Workflows
1. `contract-cli jurisdiction list` → RPC `jurisdiction.list` enumerates available packs (pulled from crate-l1 definitions).
2. `contract-cli jurisdiction set --pack US_CA` → RPC `jurisdiction.override` writes override entry scoped to account/node identity. CLI enforces signature + logging of reason codes.
3. `contract-cli jurisdiction reset` → `jurisdiction.reset` removes override, falling back to governance defaults seeded in bootstrap scripts (`docs/operations.md#bootstrap-and-configuration`).
4. `contract-cli kyc audit-log` → `node/src/rpc/identity.rs` (LE portal) surfaces aggregated logs filtered by pack ID.

## 4. Interaction with Subsystems
- **Gateway** — `node/src/gateway/policy.rs` merges jurisdiction pack settings into read/write quotas and UI warnings (JSON output consumed by law-enforcement portal). `gateway` telemetry exports `gateway_jurisdiction_overrides_total`.
- **Fees** — Packs can add per-jurisdiction surcharges consumed by `node/src/transaction/fees.rs`. The energy market (Step 2.7 settlement) queries `jurisdiction_packs` to apply local surcharges.
- **Governance** — Policy packs versions/timeframes stored as governance parameters (see `docs/economics_and_governance.md`). Upgrades go through bicameral vote as described in `03-governance-treasury.md`.
- **CLI UX** — Node CLI records localized warning strings (per AGENTS spec) in law-enforcement audit logs whenever unsupported flags appear. Implementation in `cli/src/jurisdiction.rs`.

## 5. Testing & Validation
- `crates/jurisdiction/tests/*.rs` ensure pack parsing, override precedence, and LE logging requirements hold.
- `gateway/tests/jurisdiction.rs` validates enforcement across read pathways.
- Bootstrapping scripts (`scripts/bootstrap.sh`) seed default pack selection when node first starts.

## 6. Integration Hooks for Physical Resource Layer
- Expose quota enforcement per jurisdiction for energy/bandwidth/hardware credit markets (see `06-physical-resource-layer.md`).
- Mirror LE logging requirements for new oracle adapters; each meter reading submitted must include `jurisdiction` field logged via `kyc::AuditEntry`.
- Update `docs/testnet/ENERGY_QUICKSTART.md` with pack selection instructions for energy providers once Step 3 lands.
