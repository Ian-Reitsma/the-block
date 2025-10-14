# First-Party Dependency Migration Audit

_Last updated: 2025-10-14 06:00:00Z_

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
| gateway | `node/src/gateway/read_receipt.rs` | 12 | — (manual binary cursor encode/decode) | Read receipts now encode via the first-party binary cursor helpers; serde derives removed while legacy CBOR fallback remains. |
| light_client | `node/src/light_client/proof_tracker.rs` | 11-90, 277-394 | — (manual binary cursor encode/decode) | Proof-tracker persistence moved off `binary_codec` and serde; new cursor helpers plus `util::binary_struct` cover stored relayers, claim receipts, and the legacy 8-byte fallback while keeping compatibility tests local. |
| net | `node/src/net/peer_metrics_store.rs` | 6-101 | — (manual binary cursor encode/decode) | Peer metrics sled snapshots now use `peer_metrics_binary` backed by the cursor helpers; serde derives remain for JSON/RPC exports while sled persistence is fully first-party. |
| p2p | `node/src/p2p/wire_binary.rs` | 1-360 | — (manual binary cursor encode/decode) | `WireMessage` no longer derives serde; the new `wire_binary` module encodes handshake and gossip payloads with the cursor helpers and shared `binary_struct` utilities while compatibility tests lock the legacy layout. |
| dex | `node/src/dex/storage.rs` | 1-120 | — (manual binary cursor encode/decode) | DEX order books, trade logs, escrow snapshots, and AMM pools now persist through `storage_binary`, eliminating the `binary_codec` shim and serde derives from `EscrowState` while keeping sled keys stable. |
| dex | `node/src/dex/storage_binary.rs` | 1-720 | — (manual binary cursor encode/decode) | Cursor helpers encode/decode order books, escrow state, trade logs, and pools with legacy-byte regression tests (`order_book_matches_legacy`, `trade_log_matches_legacy`, `escrow_state_matches_legacy`, `pool_matches_legacy`) and randomized coverage across order depths and escrow proofs. |
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
| storage | `node/src/storage/manifest_binary.rs` | 1-420 | — (manual binary cursor encode/decode) | Object manifests, store receipts, chunk/provider tables, and sled receipts now encode via first-party cursor helpers with regression and legacy compatibility tests, plus a randomized property suite that hammers chunk/provider variants against the legacy codec. |
| storage | `node/src/storage/pipeline.rs` | 21, 53, 213-225 | facade derive + skip/defaults | Storage pipeline manifests use `foundation_serialization::{defaults, skip}` for optionals and collections; sled persistence defers to `pipeline/binary.rs`. |
| storage | `node/src/storage/pipeline/binary.rs` | 1-220 | — (manual binary cursor encode/decode) | Provider profile sled snapshots round-trip with cursor helpers and legacy parity tests, tolerating historical payloads that lacked the newer EWMA counters while the new property harness randomizes EWMA/throughput fields to guard encoding parity. |
| storage | `node/src/storage/repair.rs` | 15, 139 | facade derive + rename_all | Repair queue tasks use facade derives with `rename_all`. |
| storage | `node/src/storage/types.rs` | 1, 19-58 | facade derive + defaults | Storage policy/state structures now reference facade defaults. |
| identity | `node/src/identity/did.rs` | 1-240 | — (manual binary cursor encode/decode) | DID registry sled persistence now routes through `identity::did_binary`, dropping `binary_codec` in favour of cursor helpers while preserving remote-attestation compatibility and replay detection. |
| identity | `node/src/identity/did_binary.rs` | 1-240 | — (manual binary cursor encode/decode) | Cursor helpers encode DID records, attestations, and optionals with legacy parity tests (including malformed-hash guards); a seeded property suite now fuzzes randomized addresses/documents and the `identity_snapshot` integration test exercises mixed legacy/current sled dumps. |
| identity | `node/src/identity/handle_registry.rs` | 1-240 | — (manual binary cursor encode/decode) | Handle registration, owner lookups, and nonce checkpoints now delegate to `identity::handle_binary`, eliminating serde-backed sled blobs while tolerating historical pq-key absence. |
| identity | `node/src/identity/handle_binary.rs` | 1-240 | — (manual binary cursor encode/decode) | Handle records, owner strings, and nonce counters round-trip through cursor helpers with compatibility fixtures covering pq-key toggles; randomized parity tests now hammer attestation lengths/nonces and the mixed-snapshot integration test verifies sled upgrades. |
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
- `node/src/identity/did.rs` and `node/src/identity/handle_registry.rs` now
  persist sled state through the first-party cursor helpers; `node/src/le_portal.rs`,
  `node/src/gossip/*`, and transaction/vm modules still derive for JSON/RPC
  exposures.
- Integration fixtures and compatibility tests now construct payloads through
  the cursor helpers (`foundation_serialization::binary_cursor`) and the shared
  `node/src/util/binary_struct.rs` routines. Peer metrics storage
  (`node/src/net/peer_metrics_store.rs`), gossip wire payloads
  (`node/src/p2p/wire_binary.rs`), and the storage sled codecs
(`node/src/storage/{manifest_binary.rs,pipeline/binary.rs,fs.rs,repair.rs}`)
now sit on the same manual path with compatibility fixtures that tolerate
historical payloads missing the modern optional fields. DEX sled persistence
(`node/src/dex/{storage.rs,storage_binary.rs}`) has joined the cursor-based
path, removing the `binary_codec` shim while regression fixtures lock order
books, escrow tables, AMM pools, and trade logs against the legacy bytes. The
residual `crate::util::binary_codec` usage survives only inside compatibility
tests while the module is phased out.

### Tooling & Support Crate Migrations (2025-10-14 update)

- ✅ `crates/sys` now ships first-party FFI shims for Linux inotify and the
  BSD/macOS kqueue family, exposes a matching epoll-backed `reactor`
  (`Poll`, `Events`, `Waker`), and adds a `sys::net` module that constructs
  non-blocking TCP/UDP sockets without touching crates.io shims. The latest
  refresh completes the Windows leg of that work: `crates/sys/src/reactor/platform_windows.rs`
  now drives an IOCP-backed backend that associates sockets with a completion
  port, fans out WSA waiters that post completions back into the queue, and
  routes runtime wakers through `PostQueuedCompletionStatus`, eliminating the
  prior 64-handle ceiling. `crates/sys/src/net/windows.rs` mirrors the Unix
  helpers via `WSASocketW`, implements `AsRawSocket`, and keeps FIRST_PARTY_ONLY
  builds free of `socket2`/`mio` even on Windows. Runtime’s watcher and
  networking stack consume these modules directly—descriptors register through
  the first-party reactor, connection handshakes ride the new socket wrappers,
  and the `runtime` crate drops `mio`, `socket2`, and `nix` entirely. FIRST_PARTY_ONLY
  builds now link watcher and networking plumbing exclusively through in-house
  code, the Linux integration suite (`crates/sys/tests/inotify_linux.rs`)
  exercises creation/deletion/directory events, the BSD harness
  (`crates/sys/tests/reactor_kqueue.rs`) validates EV_SET/EVFILT_USER wakeups,
  a new Windows scaling test (`crates/sys/tests/reactor_windows_scaling.rs`)
  guards IOCP registration growth, and the socket regression suite adds a UDP
  stress harness (`crates/sys/tests/net_udp_stress.rs`) alongside the 32-iteration
  TCP loop to keep send/recv ordering intact while the EINPROGRESS-safe
  handshake in `sys::net::TcpStream::connect` remains covered. Runtime watchers
  now consume these surfaces directly: Linux/BSD modules reuse the
  first-party inotify/kqueue shims and Windows binds to the IOCP-backed
  `DirectoryChangeDriver` with explicit `Send` guarantees plus the
  `windows-sys` feature set declared in `crates/sys/Cargo.toml`, allowing
  `FIRST_PARTY_ONLY=1 cargo check --target x86_64-pc-windows-gnu` to pass for
  both `sys` and `runtime`. Remaining `mio` references live only behind legacy
  tokio consumers slated for follow-up migration.

### Tooling & Support Crate Migrations (2025-10-12)

- ✅ Added the `foundation_serde` facade crate with a fully enumerated stub
  backend. The stub mirrors serde’s `ser`/`de` traits, visitor hierarchy,
  primitive implementations, and value helpers so FIRST_PARTY_ONLY builds can
  compile end-to-end. `foundation_serialization` now toggles backends via
  features (`serde-external`, `serde-stub`) without ever depending on upstream
  `serde` directly, and the stub backend passes `cargo check -p
  foundation_serialization --no-default-features --features serde-stub`.
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
  while we bring up a native store. The diagnostics crate no longer depends on
  the facade (or `foundation_sqlite`) after moving its storage emitters to pure
  in-house telemetry, so FIRST_PARTY_ONLY builds now link the diagnostics
  helpers without any SQLite shims.
- ✅ `crates/storage_engine` now ships an in-house JSON codec and temp-file
  harness (`json.rs`, `tempfile.rs`) that replace the old
  `foundation_serialization`/`serde` parsing layers and the `sys::tempfile`
  adapter. RocksDB, sled, and in-memory backends—plus the WAL/manifest tests—
  consume the new helpers, so FIRST_PARTY_ONLY builds no longer link external
  serialization crates when staging manifests or WAL entries.
- ✅ `crates/dependency_guard` scopes `cargo metadata` resolution to the
  requesting crate before evaluating policy violations. Guard failures now cite
  only the offending crate’s dependency graph instead of the entire workspace,
  keeping FIRST_PARTY_ONLY enforcement actionable while we migrate the
  remaining tooling crates.
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
  emit dashboards via the in-house snapshot binary
  (`monitoring/src/bin/snapshot.rs`) and
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
- ✅ Added the `http_env` helper crate and migrated CLI, node, aggregator,
  explorer, probe, jurisdiction, and example binaries to its shared TLS loader
  so HTTPS clients honour consistent prefix ordering and sink-backed
  `TLS_ENV_WARNING` events (plus observer hooks). The new `contract tls convert`
  and enhanced `contract tls stage` commands convert PEM assets into the JSON
  identities consumed by the loader, fan them out to per-service directories,
  emit canonical `--env-file` exports, allow service-specific environment
  prefix overrides, and feed both the
  `TLS_ENV_WARNING_TOTAL{prefix,code}` counter and
  `TLS_ENV_WARNING_LAST_SEEN_SECONDS{prefix,code}` gauge (with aggregator
  rehydration and retention overrides). `/tls/warnings/status` now summarizes
  retention health, the aggregator exports
  `tls_env_warning_retention_seconds`, `tls_env_warning_active_snapshots`,
  `tls_env_warning_stale_snapshots`,
  `tls_env_warning_most_recent_last_seen_seconds`, and
  `tls_env_warning_least_recent_last_seen_seconds`, plus BLAKE3 fingerprint
  gauges/counters (`tls_env_warning_detail_fingerprint{prefix,code}`,
  `tls_env_warning_variables_fingerprint{prefix,code}`,
  `tls_env_warning_detail_fingerprint_total{prefix,code,fingerprint}`,
  `tls_env_warning_variables_fingerprint_total{prefix,code,fingerprint}`) and the
  unique fingerprint gauges (`tls_env_warning_detail_unique_fingerprints{prefix,code}` /
  `tls_env_warning_variables_unique_fingerprints{prefix,code}`) so dashboards
  correlate hashed warning variants and detect novel hashes without free-form
  payloads. Monitoring ships the `TlsEnvWarningSnapshotsStale` alert, the node
  maintains a local snapshot map (`telemetry::tls_env_warning_snapshots()`) with
  per-fingerprint counts and unique tallies for on-host inspection,
  `ensure_tls_env_warning_diagnostics_bridge()` mirrors diagnostics-only log
  streams into the telemetry registry when no sinks are configured, and
  `reset_tls_env_warning_forwarder_for_testing()` keeps integration harnesses
  hermetic,
  `contract telemetry tls-warnings` mirrors that data with optional JSON/label
  filters, per-fingerprint totals, and `--probe-detail`/`--probe-variables`
  calculators, `contract tls status --latest` renders human-readable or `--json`
  automation reports with remediation hints, the aggregator logs
  `observed new tls env warning … fingerprint` on first-seen hashes, and
  `/export/all` bundles `tls_warnings/latest.json` plus
  `tls_warnings/status.json` for offline review. Grafana templates now render
  hashed fingerprint, unique-fingerprint, and five-minute delta panels, the
  Prometheus rule set adds `TlsEnvWarningNewDetailFingerprint`,
  `TlsEnvWarningNewVariablesFingerprint`, `TlsEnvWarningDetailFingerprintFlood`,
  and `TlsEnvWarningVariablesFingerprintFlood`, and the new
  `monitoring compare-tls-warnings` helper cross-checks
  `contract telemetry tls-warnings --json` against `/tls/warnings/latest` plus
  the Prometheus series so automation can flag drift automatically.
  The shared `crates/tls_warning` module now canonicalises TLS warning
  fingerprints for every consumer (node, aggregator, CLI, monitoring) and the
  aggregator exports `tls_env_warning_events_total{prefix,code,origin}` so
  dashboards can distinguish diagnostics-driven warnings from peer-ingested
  deltas without duplicating hashing logic or emitting raw payloads.
  Grafana auto-templates continue to plot "TLS env warnings (age seconds)" via
  `clamp_min(time() - max by (prefix, code)(tls_env_warning_last_seen_seconds), 0)`
  to make stale prefixes obvious, and `tls-manifest-guard` now tolerates quoted
  env-file values. The HTTP/CLI test suites exercise prefix selection, legacy
  fallbacks, canonical exports, converter round-trips, and the status workflow
  against the in-house server.
- ✅ Peer metrics exports, support-bundle smoke tests, and light-client log
  uploads route through the new `foundation_archive::{tar, gzip}` helpers,
  which now expose streaming encode/decode paths so large bundles avoid
  buffering entire payloads while staying compatible with system tooling.
- ✅ Release installers emit `.tar.gz` bundles using the same
  `foundation_archive` builders, removing the legacy `zip` dependency from the
  packaging pipeline and keeping signatures deterministic.
- ✅ CLI, explorer, wallet, and support tooling now route error handling through
  the first-party `diagnostics` crate, eliminating the workspace-wide `anyhow`
  dependency while keeping existing context and bail ergonomics intact.
- ✅ `tb-sim` exports CSV dashboards via an in-house writer, dropping the
  external `csv` crate without changing the snapshot format consumed by
  automation.
- ✅ Remote signer trace identifiers derive from a new first-party generator, so
  the wallet crate no longer links the external `uuid` implementation. A unit
  test now exercises the UUID layout and collision guarantees so log
  subscribers can rely on the string form without pulling in helper crates.
- ✅ The `xtask` lint harness switched to in-house process management, removing
  `assert_cmd`/`predicates` from dev-dependencies while still diffing git state
  and asserting JSON output through standard library helpers.
- ✅ Introduced the `foundation_metrics` facade and recorder so runtime, wallet,
  and tooling metrics no longer depend on the external `metrics`/
  `metrics-macros` crates. FIRST_PARTY_ONLY builds now route counter/histogram
  events through no-op stubs while the node installs a recorder that bridges
  those events into the existing telemetry handles.
- ✅ `metrics-aggregator` now installs the shared `AggregatorRecorder` so every
  `foundation_metrics` macro emitted across runtime backends, TLS sinks, and
  tooling surfaces flows into the Prometheus handles without reintroducing
  third-party telemetry crates. Monitoring’s snapshot CLI installs a
  lightweight `MonitoringRecorder` to expose success/error counters through the
  same facade.
- ✅ CLI binaries, explorer tooling, log indexer utilities, and runtime RPC
  clients now source `#[serde(default)]`/`skip_serializing_if` behaviour from
  `foundation_serialization::{defaults, skip}`. This keeps workspace derives on
  the facade without referencing standard-library helpers directly.
- ✅ `crates/testkit_macros` now parses serial test wrappers without the
  `syn`/`quote`/`proc-macro2` stack, keeping the serial guard fully
  first-party while preserving the existing `#[test]` ergonomics.
- ✅ `foundation_math` tests rely on new in-house floating-point assertion
  helpers (`testing::assert_close[_with]`), removing the last `approx`
  dependency from the workspace.
- ✅ Runtime no longer carries its own oneshot channel; `crates/runtime` now
  re-exports `foundation_async::sync::oneshot` and relies on the shared
  `AtomicWaker`. Companion tests in `crates/foundation_async/tests/futures.rs`
  cover join ordering, select short-circuiting, panic trapping, and oneshot
  cancellation so FIRST_PARTY_ONLY builds exercise the async facade end to end.
- ✅ Wallet binaries and the remote-signer CLI dropped the dormant `hidapi`
  feature flag; the HID placeholder still returns a deterministic error, but
  `FIRST_PARTY_ONLY` builds no longer link the native HID stack or the `cc`
  toolchain it pulled in.
- ✅ Workspace manifests now depend on the `foundation_serde` facade instead of
  crates.io `serde`, and `foundation_bigint` now provides the full in-house
  big-integer implementation so `crypto_suite` compiles without the
  `num-bigint` stack while residual `num-traits` stays with image/num-* tooling outside guard-critical paths.
- ✅ `crates/runtime` now schedules async tasks and blocking jobs through an
  in-house `WorkQueue`, removing the `crossbeam-deque`/`crossbeam-epoch`
  dependency pair from the runtime backend while retaining spawn latency and
  pending task telemetry.
- ✅ Added `crates/foundation_bigint/tests/arithmetic.rs` to exercise
  addition/subtraction/multiplication, decimal and hex parsing, shifting, and
  modular exponentiation so the in-house implementation stays locked against
  known-good vectors.

Remaining tasks before we can flip `FIRST_PARTY_ONLY=1` include replacing the
residual `serde_json` usage in deep docs/tooling (`docs/*`, `tools/`) and
wiring the remaining CLI/tooling surfaces to the `foundation_metrics`
recorder so every consumer emits first-party telemetry without bespoke shims.

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
| `serde` derives (`serde`, `serde_bytes`) | Residual derives across storage/RPC payloads (`node/src/rpc/*`) and integration fixtures | Finish porting to `foundation_serialization` proc-macros; manifests now point at the `foundation_serde` facade so derives resolve to the stub backend when `FIRST_PARTY_ONLY=1`. | Serialization Working Group — W45 |
| `bincode 1.3` | Legacy fixture helpers in `node/tests/*` and certain CLI tools | Route every binary encode/decode through `crates/codec::binary_profile()`, then gate the dependency behind a thin stub that panics if invoked after the migration window. | Codec Strike Team — W44 |
| `tar 0.4`, `flate2 1` | Snapshot/export packaging in support bundles and log archival | **Removed.** Replaced by the in-house `foundation_archive` crate (deterministic TAR writer + uncompressed DEFLATE) powering peer metrics exports, support bundles, and light-client log uploads. | Ops Tooling — W45 |
| `pqcrypto-dilithium` (optional) | PQ signature experiments behind the `quantum` feature | **Replaced.** Workspace now ships a first-party stub (`crates/pqcrypto_dilithium`) that provides deterministic keygen, sign, and verify helpers wired into the node, wallet, and commit–reveal paths. | Crypto Suite — W43 |
| `bytes 1` | Buffer utilities in networking/tests (`node/src/net/*`, benches) | `concurrency::bytes::{Bytes, BytesMut}` wrappers now back all gossip payloads and QUIC cert handling; remaining dependency is indirect via `combine` and will be stubbed next. | Networking — W44 |

The dependency guard in `node/Cargo.toml` should be updated alongside each
replacement to error out when the third-party crate is reintroduced. Tracking
issues for each item have been filed in `docs/roadmap.md` (see the "First-party
Dependency Migration" milestone) to coordinate the rollout.

### Recent Completions

- ✅ Introduced `foundation_serialization::binary_cursor::{Writer, Reader}` and
  migrated gateway read receipts onto the manual first-party encoder/decoder,
  removing the last serde derive in that surface while preserving the legacy
  CBOR fallback path.
- ✅ Expanded the storage engine test suite to cover malformed JSON, unicode
  escapes, leading-zero rejection, and temp-file persist failures so the new
  first-party codec/harness can detect regressions without third-party
  fixtures.
- ✅ The QUIC transport now uses `concurrency::DashMap` for connection caches
  and session reuse, letting us drop the external `dashmap` crate entirely.
  Session caching moved out of `foundation_tls` into the transport layer so
  the certificate helper crate no longer links `rustls`, keeping the provider
  bridge optional while `FIRST_PARTY_ONLY` builds stay clean.
- ✅ `FIRST_PARTY_ONLY` builds of the transport crate now compile with only the
  in-house and s2n adapters. Target-specific dependency gating in
  `node/Cargo.toml` drops the Quinn feature whenever the guard is enabled, and
  the session cache exposes a first-party stub when Quinn is absent so the
  resumption store no longer pulls `rustls` into first-party builds.
- ✅ FIRST_PARTY_ONLY node builds now omit the s2n transport feature entirely;
  the in-house certificate store persists DER alongside fingerprints so
  listeners can reuse certificates on restart, and `node::net::transport_quic`
  routes provider selection through the first-party adapter so handshake code
  compiles without third-party backends.
- ✅ The in-house transport cache now honours the config-driven
  `certificate_cache` path, deletes corrupt `.der` blobs instead of returning
  zeroed verifying keys, and ships integration coverage that exercises the
  override plus persistence. FIRST_PARTY_ONLY suites can isolate certificate
  artefacts without shelling environment variables, closing the remaining gap
  between test harnesses and production deployments.
- ✅ Replaced the `x509-parser` dependency with the in-house
  `transport::cert_parser` module, which performs DER parsing for Ed25519
  certificates used by the s2n backend. Certificate verification is now fully
  first party across every transport provider.
- ✅ Added first-party regression tests that assert the Quinn adapter rejects
  in-house certificates/connections and that the in-house transport records
  handshake latency, reuse, and failure metadata without external shims
  (`crates/transport/tests/provider_mismatch.rs`,
  `crates/transport/tests/inhouse.rs`).
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
- ✅ Landed the `foundation_profiler` crate, replacing the external `pprof`
  dependency with a native sampling loop and SVG renderer for profiling builds.
- ✅ Dropped the `subtle` crate by adding constant-time equality helpers to
  `crypto_suite` and wiring wallet, consensus, RPC, and DEX modules to the new
  primitives.
- ✅ Routed all workspace consumers through `sys::tempfile`, removing the
  third-party `tempfile` dependency from manifests while keeping the temporary
  directory API stable for tests and tooling. Remote signer workflows, CLI/node
  HTTP helpers, and the metrics aggregator now rely on the in-house TLS
  connector with shared environment prefixes, eliminating the lingering
  `native-tls` shim and bringing wallet and tooling HTTPS flows fully in-house.
- ✅ Rebuilt `crates/sys` on direct FFI declarations and `/proc` parsing so the
  workspace no longer depends on the upstream `libc` crate while preserving the
  existing tempfile, signal, randomness, and tty helpers under the first-party
  API surface.
- ✅ Introduced the in-house `thiserror` derive crate, replaced every workspace
  dependency on the upstream `thiserror`/`thiserror-impl` pair, and added
  regression tests so error enums now rely exclusively on the first-party macro.
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
- ✅ Added in-house Dilithium/Kyber stubs (`crates/pqcrypto_dilithium`, `crates/pqcrypto_kyber`) so `quantum`/`pq` builds no longer pull the crates.io PQ stack. CLI, wallet, governance, and commit–reveal flows now sign and encapsulate via the first-party helpers while keeping deterministic encodings for tests and telemetry.
- ✅ Replaced the external `serde_bytes` crate with `foundation_serialization::serde_bytes`, retaining the `#[serde(with = "serde_bytes")]` annotations while keeping byte buffers on first-party serializers. Node read receipts and exec payloads now round-trip without the third-party shim.
