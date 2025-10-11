# First-Party Dependency Migration Audit

_Last updated: 2025-10-10 22:45:00Z_

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
| compute_market | `node/src/compute_market/mod.rs` | 5, 57, 81-87, 126, 250, 255 | `foundation_serialization::{Deserialize, Serialize}` derive + facade defaults | Lane policy/state structs now pull defaults/skip handlers from `foundation_serialization::{defaults, skip}`. |
| compute_market | `node/src/compute_market/cbm.rs` | 1 | facade derive | CBM configuration round-trips via the facade re-export. |
| compute_market | `node/src/compute_market/courier.rs` | 6 | facade derive | Courier payloads retain facade derives; persistence already uses `foundation_serialization::binary::{encode, decode}`. |
| compute_market | `node/src/compute_market/courier_store.rs` | 1 | facade derive | Receipt store persists via `foundation_serialization::binary::{encode, decode}` for sled values. |
| compute_market | `node/src/compute_market/errors.rs` | 1 | `foundation_serialization::Serialize` | Error surfaces expose facade serialization for RPC. |
| compute_market | `node/src/compute_market/price_board.rs` | 3 | facade derive | Price board structs derive through the facade and snapshot via `foundation_serialization::binary`. |
| compute_market | `node/src/compute_market/receipt.rs` | 3 | facade derive + optional defaults | Receipt encoding now references `foundation_serialization::defaults::default` and `foundation_serialization::skip::option_is_none`. |
| compute_market | `node/src/compute_market/scheduler.rs` | 3, 24-36, 849 | facade derive + defaults | Scheduler capability/reputation state uses facade helpers for defaults. |
| compute_market | `node/src/compute_market/settlement.rs` | 22, 62 | facade derive (`foundation_serialization::de::DeserializeOwned`) | Settlement pipeline routes SimpleDb blobs through the facade; optional fields use the facade skip helpers. |
| compute_market | `node/src/compute_market/workload.rs` | 1 | facade derive | Workload manifests serialize via the facade exports. |
| storage | `node/src/storage/fs.rs` | 6 | facade derive | Filesystem escrow entries serialize through the facade. |
| storage | `node/src/storage/pipeline.rs` | 21, 53, 213-225 | facade derive + skip/defaults | Storage pipeline manifests use `foundation_serialization::{defaults, skip}` for optionals and collections. |
| storage | `node/src/storage/repair.rs` | 15, 139 | facade derive + rename_all | Repair queue tasks use facade derives with `rename_all`. |
| storage | `node/src/storage/types.rs` | 1, 19-58 | facade derive + defaults | Storage policy/state structures now reference facade defaults. |
| governance | `node/src/governance/mod.rs` | 35 | facade derive | Module-level envelope derives via the facade. |
| governance | `node/src/governance/bicameral.rs` | 2 | facade derive | Bicameral state persists via facade derives. |
| governance | `node/src/governance/inflation_cap.rs` | 8 | `foundation_serialization::Serialize` | Inflation cap reports export using the facade serializer. |
| governance | `node/src/governance/params.rs` | 15, 138, 163-167, 996-997 | facade derive + defaults | `EncryptedUtilization::decrypt` decodes with the facade; structs use facade default helpers. |
| governance | `node/src/governance/proposals.rs` | 2 | facade derive | Proposal DAG nodes rely on the facade re-export. |
| governance | `node/src/governance/release.rs` | 2 | facade derive | Release policy serializes via the facade helpers. |
| governance | `node/src/governance/state.rs` | 1 | facade derive | Global governance state uses the facade. |
| governance | `node/src/governance/store.rs` | 15, 45, 47 | facade derive + skip helpers | Persistence routes through `foundation_serialization::binary::{encode, decode}` with facade skip predicates. |
| governance | `node/src/governance/token.rs` | 2 | facade derive | Token accounting uses facade derives. |
| governance | `node/src/governance/kalman.rs` | 1 | serde derive (first-party math) | Kalman filter now uses `foundation_math` vectors/matrices and `ChiSquared`; serde derives limited to struct definitions. |
| governance | `node/src/governance/variance.rs` | (see §2) | — | Burst veto DCT now routes through `foundation_math::transform::dct2_inplace`. |
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
- Integration fixtures now construct payloads through
  `crate::util::binary_codec`, keeping the shared helpers in one place and
  routing encode/decode operations through the facade-backed metrics hooks.

### Tooling & Support Crate Migrations (2025-10-10)

- ✅ `crates/jurisdiction` now signs, fetches, and diffs policy packs via
  `foundation_serialization::json` and the in-house HTTP client, eliminating the
  `ureq` dependency entirely.
- ✅ Governance webhook notifications dispatch through `httpd::BlockingClient`
  so telemetry alerts ride first-party HTTP primitives instead of `ureq`.
- ✅ `tb-sim`'s dependency-fault harness defaults to the first-party binary codec;
  the CLI no longer advertises `--codec bincode`, keeping harness telemetry in
  sync with the renamed binary profiles.
- ✅ Lightweight probes (`tools/probe.rs`, `tools/partition_probe.rs`) fetch
  metrics over raw `TcpStream` requests, dropping their previous reliance on the
  `ureq` crate.
- ✅ `crates/probe` emits RPC payloads through the in-house `json!` macro, and
  `crates/wallet` (including the remote signer tests) round-trips signer
  messages with the same facade.
- ✅ Introduced the `foundation_sqlite` facade so CLI, explorer, and log/indexer
  tooling share first-party parameter/value handling. The facade defaults to the
  `rusqlite` engine but exposes a stub backend for `FIRST_PARTY_ONLY` builds
  while we bring up a native store.
- ✅ `sim/` (core harness, dependency-fault metrics, chaos summaries, and DID
  generator) serializes exclusively with the facade, while globals continue to
  rely on `foundation_lazy` for deterministic initialization.
- ✅ `examples/mobile`, `examples/cli`, and the wallet remote signer demo all
  consume the shared JSON helpers so downstream automation builds no longer
  pull in `serde_json`.
- ✅ `crates/codec` now wraps the facade’s JSON implementation and ships a
  first-party binary profile, removing the lingering `serde_cbor` dependency
  from production crates.
- ✅ `crates/light-client` device telemetry and state snapshot metrics now feed
  the in-house `runtime::telemetry` registry, removing the optional
  `prometheus` dependency and updating regression tests to assert against the
  first-party collector snapshots.
- ✅ Monitoring scripts, docker-compose assets, and the metrics aggregator all
  emit dashboards via `monitoring/tools/render_foundation_dashboard.py` and
  `httpd::metrics::telemetry_snapshot`, removing Prometheus from the
  observability toolchain. Wrapper summaries inside the aggregator now rely on
  `foundation_serialization::defaults::default` so telemetry exports stay on
  first-party helpers.
- ✅ `tools/analytics_audit` decodes read acknowledgement batches with the
  facade’s binary helpers, ensuring telemetry audits stay on first-party
  codecs while retaining the Merkle validation workflow.
- ✅ `tools/gov_graph` now reads proposal DAG entries via
  `foundation_serialization::binary::decode`, removing the final `bincode`
  usage from the governance DOT export helper.
- ✅ Peer metrics exports, support-bundle smoke tests, and light-client log
  uploads route through the new `foundation_archive::{tar, gzip}` helpers,
  which now expose streaming encode/decode paths so large bundles avoid
  buffering entire payloads while staying compatible with system tooling.
- ✅ Release installers emit `.tar.gz` bundles using the same
  `foundation_archive` builders, removing the legacy `zip` dependency from the
  packaging pipeline and keeping signatures deterministic.
- ✅ CLI binaries, explorer tooling, log indexer utilities, and runtime RPC
  clients now source `#[serde(default)]`/`skip_serializing_if` behaviour from
  `foundation_serialization::{defaults, skip}`. This keeps workspace derives on
  the facade without referencing standard-library helpers directly.

Remaining tasks before we can flip `FIRST_PARTY_ONLY=1` include replacing the
residual `serde_json` usage in deep docs/tooling (`docs/*`, `tools/`) and
landing the last telemetry histogram adapters in `runtime::telemetry` so
tooling can model quantiles without local patches.

## 2. Third-Party Math, FFT, and Parallelism Inventory

| Crate | Functionality | Primary Call Sites | Notes |
| --- | --- | --- | --- |
| `nalgebra` | Dense linear algebra for Kalman filter state (`DVector`, `DMatrix`) | — | **Removed.** Replaced by `crates/foundation_math::linalg` fixed-size matrices/vectors powering both node and governance Kalman filters. |
| `statrs` | Statistical distributions (Chi-squared CDF) for Kalman confidence bounds | — | **Removed.** Replaced by `crates/foundation_math::distribution::ChiSquared` inverse CDF implementation. |
| `foundation_math` | First-party linear algebra & distributions | `node/src/governance/kalman.rs`; `node/src/governance/params.rs`; `governance/src/{kalman,params}.rs` | Provides fixed-size matrices/vectors plus chi-squared quantiles used by Kalman retuning; extend with DCT/backoff primitives next. |
| `rustdct` | Fast cosine transform planner for variance smoothing | — | **Removed.** Replaced by `foundation_math::transform::dct2_inplace`, wiring node and governance burst veto logic to first-party code. |
| `rayon` | Parallel iterators and thread pool | — | **Removed.** Conflict-aware task scheduling and storage repair batches now execute on scoped std threads, eliminating the external pool. |
| `bytes` | — | _No active call sites in node/runtime crates_ | `bytes` crate no longer imported in production modules; manifests may still include it indirectly (verify before removal). |

### Supporting Crates Mirroring Runtime Usage

- Governance standalone crate mirrors the node governance modules, so any
  migration must update both `node/` and `governance/` to keep shared logic in
  sync.
- `crates/codec` and `crates/crypto_suite` now forward their transaction
  profiles to the first-party binary facade. Keep the panic-on-use guard for
  `FIRST_PARTY_ONLY=1` until downstream crates migrate to the new
  `BinaryProfile` aliases.

## 3. Next Steps Toward Full Migration

1. **Finalize facade ergonomics:** now that defaults/skip helpers are wired
   across runtime/tooling crates, capture any bespoke predicates (e.g., non-zero
   numeric guards) inside `foundation_serialization` so downstream code stops
   shadowing std helpers entirely. Promote derive macros once coverage is
   exhaustive.
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

## 4. Stub Backlog for FIRST_PARTY_ONLY Builds

The handle migration eliminated direct collector access across the node and
ancillary tooling, but several third-party crates still block
`FIRST_PARTY_ONLY=1` builds. The highest-impact items to stub are:

| Crate | Primary Consumers | Notes |
| --- | --- | --- |
| `rusqlite` | `cli`, `explorer`, `tools/{indexer,log_indexer_cli}` | ✅ Direct call-sites now route through the new `foundation_sqlite` facade. The default `rusqlite-backend` feature supplies the engine for standard builds while `FIRST_PARTY_ONLY` compiles against the stub that returns backend-unavailable errors. Follow-up: replace the backend shim with an in-house engine. |
| `sled` | `the_block::SimpleDb`, storage tests, log indexer | Runtime already wraps sled; deliver an in-house key-value engine stub (even if backed by sled) so `FIRST_PARTY_ONLY` can compile without linking the crate. |
| `openssl`/`openssl-sys` | transitive via TLS tooling | QUIC/TLS stacks still pull these in when the bundled providers are enabled. Scope a lightweight first-party crypto shim (or the minimal FFI needed for mutual TLS) so the guard can be satisfied without OpenSSL. |

Each stub should follow the telemetry handle pattern: provide the API surface at
build time, emit targeted diagnostics when functionality is unavailable, and
gate the full implementation behind a feature flag so we can ship both
first-party-only and full-stack builds without code churn.

This audit should unblock targeted migration work by providing a definitive
reference for remaining third-party dependency usage within the node runtime
and governance stacks.

## 4. Outstanding First-Party Stub Requirements

The table below captures every third-party dependency that still blocks a
`FIRST_PARTY_ONLY=1` build. Each entry lists the primary call sites and the
stub/replacement strategy that should be scheduled next. Owners reflect the
responsible subsystem leads from the roadmap; timelines assume two-week
delivery windows unless otherwise specified.

| Dependency | Current Usage (Representative Modules) | Stub / Replacement Plan | Owner & Timeline |
| --- | --- | --- | --- |
| `serde` derives (`serde`, `serde_bytes`) | Residual derives across gateway/storage RPCs (`node/src/gateway/read_receipt.rs`, `node/src/rpc/*`) and integration fixtures | Finish porting to `foundation_serialization` proc-macros; add facade shim that exposes `derive(Serialize, Deserialize)` when `FIRST_PARTY_ONLY=1` so crates compile without the external crate. | Serialization Working Group — W45 |
| `bincode 1.3` | Legacy fixture helpers in `node/tests/*` and certain CLI tools | Route every binary encode/decode through `crates/codec::binary_profile()`, then gate the dependency behind a thin stub that panics if invoked after the migration window. | Codec Strike Team — W44 |
| `subtle 2` | Constant-time comparisons in the wallet/identity stacks | Inline constant-time primitives inside `crypto_suite` (`ct_equal`, `ct_assign`) and drop the dependency once verification coverage lands. | Crypto Suite — W43 |
| `tar 0.4`, `flate2 1` | Snapshot/export packaging in support bundles and log archival | **Removed.** Replaced by the in-house `foundation_archive` crate (deterministic TAR writer + uncompressed DEFLATE) powering peer metrics exports, support bundles, and light-client log uploads. | Ops Tooling — W45 |
| `pqcrypto-dilithium` (optional) | PQ signature experiments behind the `quantum` feature | Mirror Dilithium inside `crypto_suite::pq` (or stub to panic) and gate the external crate behind `FIRST_PARTY_ONLY=0` until the in-house implementation lands. | Crypto Suite — W48 |
| `pprof 0.13` | Flamegraph dumps for profiling harnesses | Offer a `foundation_profiler` crate that emits the same file format using our sampling hooks; stub out the external crate when profiling is disabled. | Performance Guild — W46 |
| `bytes 1` | Buffer utilities in networking/tests (`node/src/net/*`, benches) | `concurrency::bytes::{Bytes, BytesMut}` wrappers now back all gossip payloads and QUIC cert handling; remaining dependency is indirect via `combine` and will be stubbed next. | Networking — W44 |

The dependency guard in `node/Cargo.toml` should be updated alongside each
replacement to error out when the third-party crate is reintroduced. Tracking
issues for each item have been filed in `docs/roadmap.md` (see the "First-party
Dependency Migration" milestone) to coordinate the rollout.

### Recent Completions

- ✅ Replaced the third-party `lru` crate with `concurrency::cache::LruCache`
  and rewired the node/explorer caches to the in-house implementation, removing
  another blocker for `FIRST_PARTY_ONLY=1` builds.
- ✅ Eliminated `indexmap` by introducing `concurrency::collections::OrderedMap`
  and migrating the peer metrics registry and dependency tooling onto the
  first-party ordered map implementation.
- ✅ Introduced `foundation_regex` and migrated CLI/net filtering to the
  deterministic in-house engine, removing the workspace dependency on
  `regex`/`regex-automata`/`regex-syntax`.
- ✅ Added `sys::tty::dimensions()` and switched CLI layout heuristics to the
  first-party helper so the `terminal_size` crate is no longer required.
- ✅ Landed the `foundation_time` crate so runtime storage repair logs,
  metrics snapshots, and QUIC certificate rotation no longer depend on the
  third-party `time` API, and replaced the remaining rcgen bridge with the
  first-party `foundation_tls` certificate builder.
- ✅ Introduced `foundation_unicode` so handle normalization and identity flows
  no longer rely on the ICU normalizer or its data tables; callers now share a
  first-party NFKC + ASCII fast-path implementation.
- ✅ Replaced the external `hex` crate with `crypto_suite::hex` helpers, updating
  CLI, node, explorer, wallet, and tooling call sites to the first-party
  encoder/decoder and removing the dependency from workspace manifests.
- ✅ Introduced the `foundation_tui` crate so CLI tooling now uses the
  in-house colour helpers, allowing us to drop the third-party `colored`
  dependency from the node binary and the wider workspace manifests.
- ✅ Rewrote the `tools/xtask` diff helper to shell out to git, letting us drop
  the `git2` bindings and their `url`/`idna_adapter`/ICU stack from the
  workspace manifests.
- ✅ Migrated the gateway fuzz harness onto the first-party HTTP parser and
  dropped the `httparse` dependency from the workspace.
- ✅ Implemented `concurrency::filters::xor8::Xor8`, rewired the rate-limit
  filter, and removed the `xorfilter-rs` crate.
