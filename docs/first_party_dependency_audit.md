# First-Party Dependency Migration Audit

_Last updated: 2025-10-09 15:20:00Z_

This document tracks remaining third-party serialization and math/parallelism
usage across the production-critical surfaces requested in the umbrella
migration tasks. It complements the workspace-level dependency inventory and
narrows the focus to call sites that must be migrated onto the
`foundation_serialization`, `codec`, and in-house math primitives.

## 1. Serialization Touchpoints in Node Runtime Modules

The table below enumerates every `serde`, `serde_json`, or `bincode`
interaction under `node/src/{gateway,compute_market,storage,governance,rpc}`.
Derive annotations (`#[derive(Serialize, Deserialize)]`, `#[serde(...)]`) are
listed alongside any runtime helper calls to make migration sequencing
explicit.

| Module | File | Line(s) | Third-party usage | Notes |
| --- | --- | --- | --- | --- |
| gateway | `node/src/gateway/read_receipt.rs` | 12 | `serde::{Deserialize, Serialize}` derive | Receipt envelopes and gateway attestations remain on serde derives; no facade wrapper yet. |
| compute_market | `node/src/compute_market/mod.rs` | 5, 57, 81-87, 126, 250, 255 | `serde::{Deserialize, Serialize}` derive + field attributes | Lane policy/state structs use serde rename/default attributes. |
| compute_market | `node/src/compute_market/cbm.rs` | 1 | serde derive | CBM configuration round-trips via serde. |
| compute_market | `node/src/compute_market/courier.rs` | 6 | serde derive | Courier payloads retain serde derives, but persistence now calls `foundation_serialization::binary::{encode, decode}` instead of `bincode`. |
| compute_market | `node/src/compute_market/courier_store.rs` | 1 | serde derive | Receipt store now persists via `foundation_serialization::binary::{encode, decode}` for sled values; serde derives remain on `Receipt`. |
| compute_market | `node/src/compute_market/errors.rs` | 1 | `serde::Serialize` | Error surfaces expose serde serialization for RPC. |
| compute_market | `node/src/compute_market/price_board.rs` | 3 | serde derive | Price board structs still derive serde, yet snapshot persistence now reuses the in-house binary codec (`foundation_serialization::binary`). |
| compute_market | `node/src/compute_market/receipt.rs` | 3 | serde derive | Receipt encoding still serde-based; includes optional field handling. |
| compute_market | `node/src/compute_market/scheduler.rs` | 3, 24-36, 849 | serde derive + defaults | Scheduler policy config uses serde default helpers. |
| compute_market | `node/src/compute_market/settlement.rs` | 22, 62 | serde derive (with `serde::de::DeserializeOwned`) | Settlement pipeline now routes SimpleDb blobs through `foundation_serialization::binary`; serde derives remain for struct definitions. |
| compute_market | `node/src/compute_market/workload.rs` | 1 | serde derive | Workload manifests serialized/deserialized via serde. |
| storage | `node/src/storage/fs.rs` | 6 | serde derive | Filesystem manifest uses serde. |
| storage | `node/src/storage/pipeline.rs` | 21, 53, 213-225 | serde derive + skip/defaults | Storage pipeline manifests rely on serde attribute logic. |
| storage | `node/src/storage/repair.rs` | 15, 139 | serde derive + rename_all | Repair queue tasks use serde rename for enums. |
| storage | `node/src/storage/types.rs` | 1, 19-58 | serde derive + defaults | Storage policy/state structures remain serde-backed. |
| governance | `node/src/governance/mod.rs` | 35 | serde derive | Module-level envelope still on serde derives. |
| governance | `node/src/governance/bicameral.rs` | 2 | serde derive | Bicameral state persists via serde. |
| governance | `node/src/governance/inflation_cap.rs` | 8 | `serde::Serialize` | Inflation cap reports export using serde. |
| governance | `node/src/governance/params.rs` | 15, 138, 163-167, 996-997 | serde derive + defaults | `EncryptedUtilization::decrypt` now decodes with `foundation_serialization::binary`; remaining structs still derive via serde. |
| governance | `node/src/governance/proposals.rs` | 2 | serde derive | Proposal DAG nodes rely on serde. |
| governance | `node/src/governance/release.rs` | 2 | serde derive | Release policy still serialized through serde. |
| governance | `node/src/governance/state.rs` | 1 | serde derive | Global governance state depends on serde. |
| governance | `node/src/governance/store.rs` | 15, 45, 47 | serde derive + skip_serializing_if | Persistence now routes through `foundation_serialization::binary::{encode, decode}` instead of `bincode`. |
| governance | `node/src/governance/token.rs` | 2 | serde derive | Token accounting uses serde. |
| governance | `node/src/governance/kalman.rs` | 1 | serde derive (first-party math) | Kalman filter now uses `foundation_math` vectors/matrices and `ChiSquared`; serde derives limited to struct definitions. |
| governance | `node/src/governance/variance.rs` | (see §2) | — | (See math section for rustdct usage.) |
| governance | `node/src/governance` (misc) | — | `serde_json` — none observed | Runtime crate already routes JSON through facade; governance code has no direct serde_json usage. |
| rpc | `node/src/rpc/mod.rs` | 21-32, 364-620 | **Migrated to `foundation_rpc` request/response envelope** | Runtime handlers now parse via the first-party `foundation_rpc` crate; remaining serde derives only cover auxiliary payload structs. |
| rpc | `node/src/rpc/client.rs` | 1-340 | serde derive + skip/default bounds | Client helpers now emit/parse `foundation_rpc::{Request, Response}` envelopes but still deserialize typed payloads through serde. |

> **New first-party RPC facade:** the `foundation_rpc` crate now anchors the
> workspace-wide request/response schema, allowing `jsonrpc-core` to be removed
> from manifests while keeping CLI and runtime handlers on a shared, audited
> envelope.
| rpc | `node/src/rpc/analytics.rs` | 3 | serde derive | Analytics endpoints encode serde payloads. |
| rpc | `node/src/rpc/light.rs` | 2, 17, 43 | serde serialize + skip attributes | Light-client responses rely on serde. |
| rpc | `node/src/rpc/logs.rs` | 9 | serde serialize | Log export stream uses serde for structured frames. |

\* `kalman.rs` now consumes `foundation_math` matrices/vectors; serde derives
remain only for persistence until bespoke facade derives land.

### Non-node Call Sites Worth Tracking

While outside the strict module list, several adjacent surfaces still call into
third-party codecs:

- `node/src/telemetry.rs` and `node/src/telemetry/summary.rs` expose serde
  serialization for metrics payloads.
- `node/src/identity/*`, `node/src/le_portal.rs`, `node/src/gossip/*`, and
  transaction/vm modules retain serde derives for persisted state.
- Integration tests under `node/tests/` perform direct `bincode::serialize`
  and `bincode::deserialize` calls to construct fixtures. These will need to be
  swapped to the canonical profiles exposed by `crates/codec` when the facade is
  widened.

### Tooling & Support Crate Migrations (2025-10-09)

- ✅ `crates/jurisdiction` now signs, fetches, and diffs policy packs via
  `foundation_serialization::json`, allowing the `ureq` JSON feature and
  `serde_json` dependency to be removed.
- ✅ `crates/probe` emits RPC payloads through the in-house `json!` macro, and
  `crates/wallet` (including the remote signer tests) round-trips signer
  messages with the same facade.
- ✅ `sim/` (core harness, dependency-fault metrics, chaos summaries, and DID
  generator) serializes exclusively with the facade, while globals continue to
  rely on `foundation_lazy` for deterministic initialization.
- ✅ `examples/mobile`, `examples/cli`, and the wallet remote signer demo all
  consume the shared JSON helpers so downstream automation builds no longer
  pull in `serde_json`.
- ✅ `crates/codec` now wraps the facade’s JSON implementation, exposing the
  same API surface without depending on third-party encoders.
- ✅ `crates/light-client` device telemetry and state snapshot metrics now feed
  the in-house `runtime::telemetry` registry, removing the optional
  `prometheus` dependency and updating regression tests to assert against the
  first-party collector snapshots.
- ✅ Monitoring scripts, docker-compose assets, and the metrics aggregator all
  emit dashboards via `monitoring/tools/render_foundation_dashboard.py` and
  `httpd::metrics::telemetry_snapshot`, removing Prometheus from the
  observability toolchain.

Remaining tasks before we can flip `FIRST_PARTY_ONLY=1` include replacing the
residual `serde_json` usage in deep docs/tooling (`docs/*`, `tools/`), finishing
the bincode retirement plan for integration fixtures, and landing the last
telemetry histogram adapters in `runtime::telemetry` so tooling can model
quantiles without local patches.

## 2. Third-Party Math, FFT, and Parallelism Inventory

| Crate | Functionality | Primary Call Sites | Notes |
| --- | --- | --- | --- |
| `nalgebra` | Dense linear algebra for Kalman filter state (`DVector`, `DMatrix`) | — | **Removed.** Replaced by `crates/foundation_math::linalg` fixed-size matrices/vectors powering both node and governance Kalman filters. |
| `statrs` | Statistical distributions (Chi-squared CDF) for Kalman confidence bounds | — | **Removed.** Replaced by `crates/foundation_math::distribution::ChiSquared` inverse CDF implementation. |
| `foundation_math` | First-party linear algebra & distributions | `node/src/governance/kalman.rs`; `node/src/governance/params.rs`; `governance/src/{kalman,params}.rs` | Provides fixed-size matrices/vectors plus chi-squared quantiles used by Kalman retuning; extend with DCT/backoff primitives next. |
| `rustdct` | Fast cosine transform planner for variance smoothing | `node/src/governance/variance.rs`; `governance/src/variance.rs` | Provides type-2 DCT via planner; evaluate migrating to in-house FFT/DCT in `crates/coding` or bespoke implementation. |
| `rayon` | Parallel iterators and thread pool | `node/src/parallel.rs`; `node/src/storage/repair.rs` (thread pool + parallel iterators) | Node runtime still depends on rayon for CPU-bound pipelines; requires replacement with internal thread-pool implementation. |
| `bytes` | — | _No active call sites in node/runtime crates_ | `bytes` crate no longer imported in production modules; manifests may still include it indirectly (verify before removal). |

### Supporting Crates Mirroring Runtime Usage

- Governance standalone crate mirrors the node governance modules, so any
  migration must update both `node/` and `governance/` to keep shared logic in
  sync.
- `crates/codec` and `crates/crypto_suite` currently wrap canonical bincode
  configurations; future binary adapters should be added here to preserve API
  parity while removing the external `bincode` dependency.

## 3. Next Steps Toward Full Migration

1. **Extend `foundation_serialization`:** add JSON/binary adapters that emulate
   the serde defaults currently relied upon (e.g., default/skip semantics for
   optionals). Introduce helper derive macros or manual `impl Serialize`/`impl
   Deserialize` via the facade to unblock compute-market/storage/governance
   structs.
2. **Refactor Call Sites:** replace direct serde derives with the new helpers,
   ensuring deterministic round-trips. Update persistence layers and RPC
   handlers to consume the facade APIs instead of `serde_json` or `bincode`.
3. **Fixture Updates:** port test fixtures to use `crates/codec` (or new
   first-party binary encoders) and run `FIRST_PARTY_ONLY=0 cargo test -p
   the_block` after each migration stage.
4. **Math/FFT/Parallelism Replacement:** design in-house primitives under
   `crates/coding` or a new math crate to cover matrix algebra, chi-squared CDF,
   and DCT operations. Wire node/governance modules to the replacements and drop
   the third-party crates from manifests, then benchmark the new stacks.

This audit should unblock targeted migration work by providing a definitive
reference for remaining third-party dependency usage within the node runtime
and governance stacks.
