# System-Wide Economic Changes
> **Review (2025-10-14, late evening+++):** Dependency registry automation now
> publishes a signed drift summary and monitoring surfaces. The CLI runner emits
> `dependency-check.summary.json` beside telemetry/violations, the release
> provenance script hashes the summary, telemetry, and metrics artefacts before
> signing, and `tools/xtask` prints the parsed verdict so CI preflights fail fast
> on drift. Monitoring’s metric catalogue, Grafana templates, and alert rules now
> include dependency-policy panels plus `dependency_registry_check_status`
> alerts when drift or stale snapshots appear, and the compare utility builds on
> the first-party serialization alias so FIRST_PARTY_ONLY checks stay clean.
> **Review (2025-10-14, pre-dawn++):** Log archive key rotation now writes to
> sled with a rollback guard—updated entries only replace the originals once the
> full batch re-encrypts successfully, and any storage error rewrites the staged
> ciphertext back to its prior form before flushing so the log store never lands
> in a mixed-key state. The dependency registry CLI gained a dedicated runner
> that resolves config overrides, emits every artifact (registry JSON,
> violations, telemetry, manifest, snapshot), honours the
> `TB_DEPENDENCY_REGISTRY_DOC_PATH` override for tests, and returns the
> `RunArtifacts` struct for automation. A new end-to-end CLI test exercises that
> runner against the fixture workspace, asserting on JSON payloads, telemetry
> counters, snapshot emission, and manifest content without mutating repo docs.
> The registry parser test suite now feeds a complex `cargo metadata` fixture
> featuring optional, git, and duplicate edges to lock in adjacency deduping,
> reverse dependency tracking, and origin detection across uncommon graph
> layouts.
> **Review (2025-10-14, late night+):** The dependency registry CLI now keeps
> policy loading, registry modelling, and snapshot emission entirely within the
> serialization facade. TOML configs parse through the new
> `foundation_serialization::toml::parse_table` helper, tier/license/settings
> sections normalise manually, and JSON payloads convert via handwritten Value
> builders plus `json::to_vec_value`, removing serde while ensuring stub builds
> exercise the full suite with new regression tests.
> **Review (2025-10-14, late night):** Log archive key rotation now decrypts and
> stages every entry before writing so failures never leave the sled store
> half-migrated; the suite gained an atomic rotation regression test and the
> JSON probe now round-trips a full `LogEntry` so FIRST_PARTY_ONLY runs skip when
> the stub facade is active. The dependency registry CLI drops the
> `cargo_metadata`/`camino` crates, invoking `cargo metadata` directly and
> parsing the graph through the in-house JSON facade with new unit and
> integration coverage that auto-skip on the stub backend.
> **Review (2025-10-14, afternoon):** Serialization for TLS operations is fully
> first-party. The `foundation_serde` stub backend now mirrors serde’s visitor
> hierarchy (options, tuples, arrays, maps, sequences),
> `foundation_serialization::json::Value` implements manual serde parity, and
> the CLI’s TLS types were rewritten with handwritten serializers/deserializers
> that respect the historic snake-case/origin rules while skipping optional
> fields manually. As a result the TLS manifest/status workflows round-trip
> cleanly on FIRST_PARTY_ONLY builds and the CLI’s TLS test suite passes without
> third-party derives. Supporting cleanup tightened node defaults: aggregator
> and QUIC configs call the shared default helpers, engine selection uses the
> same `default_engine_kind()` hook we expose to serde, peer reputation records
> now seed timers via `instant_now()`, compute-market offers expose an
> `effective_reputation_multiplier()` helper, and the storage pipeline’s binary
> encoder validates field counts through the cursor helpers so `LengthOverflow`
> is exercised instead of sitting dormant. These changes eliminate the lingering
> dead-code/unnecessary-import warnings in `node/src`, keeping guard runs and
> CI noise-free while preserving the TLS automation introduced earlier in the
> week. Follow-up regression tests now lock those paths in place:
> `cli/src/tls.rs` adds JSON round-trip coverage for warning status/snapshot
> payloads, `crates/foundation_serialization/tests/json_value.rs` verifies the
> manual `Value` encoder handles nested objects and non-finite float rejection,
> and `node/src/storage/pipeline/binary.rs` exercises the overflow guard via
> `write_field_count_rejects_overflow` so future encoder tweaks stay audited.
> **Review (2025-10-14, mid-morning):** Terminal prompting now runs fully on
> first-party code with regression coverage. `sys::tty` routes passphrase reads
> through a reusable helper that toggles echo guards and trims trailing CR/LF,
> and the module picked up unit tests that drive the logic via in-memory
> streams. `foundation_tui::prompt` layers override hooks so downstream crates
> can inject scripted responses, and `contract-cli`’s log flows now include unit
> tests that exercise optional/required prompting without third-party
> dependencies. Together they keep FIRST_PARTY_ONLY builds interactive-friendly
> while guarding regressions.
> **Review (2025-10-14, late night):** Runtime watchers across Linux, BSD, and
> Windows now sit entirely on the new first-party surfaces. `crates/runtime/src/fs/watch.rs`
> reintroduces the inotify and kqueue modules atop `sys::inotify`/`sys::kqueue`
> while wiring the Windows watcher to the IOCP-backed
> `DirectoryChangeDriver` from `crates/sys/src/fs/windows.rs`. The driver and
> completion context now implement `Send` so the blocking worker satisfies the
> runtime’s `spawn_blocking` bounds, and `crates/sys/Cargo.toml` declares the
> `windows-sys` feature set needed for cross-target builds.
> **Review (2025-10-14, evening):** Mobile device probes now rely purely on
> first-party code. The iOS probe (`crates/light-client/src/device/ios.rs`)
> issues Objective-C messages and CoreFoundation queries through local FFI
> shims, removing the `objc`, `objc-foundation`, `objc_id`, and
> `core-foundation` crates. Android’s probe delegates to
> `sys::device::{battery,network}` for charging, capacity, and Wi-Fi checks
> sourced from `/sys/class/power_supply` and `/proc/net/wireless`, eliminating
> the `jni`, `ndk`, and `ndk-context` stacks and exposing reusable helpers for
> other tooling.
> **Review (2025-10-14):** The `sys` crate now exports first-party FFI shims for
> Linux inotify, BSD/macOS kqueue, and an IOCP-backed Windows reactor. The
> updated `crates/sys/src/reactor/platform_windows.rs` associates every socket
> with a completion port, fans out WSA event waiters across shards that post
> completions back into the queue, and routes waker triggers through
> `PostQueuedCompletionStatus` so descriptors, timers, and manual wake-ups share
> one scalable path without the prior 64-handle ceiling. TCP/UDP constructors
> under `sys::net::{unix,windows}` remain first party, while the Windows module
> now implements `AsRawSocket` so higher layers can register handles without any
> shim crates. The `runtime` crate drops `mio`, `socket2`, and `nix` entirely;
> watcher plumbing now consumes the platform-specific drivers—`sys::inotify`
> on Linux, `sys::kqueue` on BSD/macOS, and the IOCP `DirectoryChangeDriver` on
> Windows—so all targets share the first-party reactor. The
> Linux integration suite (`crates/sys/tests/inotify_linux.rs`) and BSD harness
> (`crates/sys/tests/reactor_kqueue.rs`) continue to exercise recursive and
> waker paths, while the socket stress suite pairs the 32-iteration TCP harness
> with a UDP bidirectional loop (`crates/sys/tests/net_udp_stress.rs`) to guard
> send/recv ordering across platforms. CI and local automation now run
> `FIRST_PARTY_ONLY=1 cargo check --target x86_64-pc-windows-gnu` for the `sys`
> and `runtime` crates so Windows regressions are caught alongside Linux builds
> without reintroducing third-party dependencies.
> **Review (2025-10-12):** Replaced the runtime scheduler’s
> `crossbeam-deque`/`crossbeam-epoch` work queues with a first-party
> `WorkQueue` that backs both async tasks and the blocking pool while keeping
> spawn latency/pending task telemetry intact. Added regression coverage for
> the `foundation_bigint` engine so arithmetic, parsing, shifting, and modular
> exponentiation now lock the in-house implementation against deterministic
> vectors.
> **Review (2025-10-12):** Introduced the `foundation_serde` facade and stub
> backend so FIRST_PARTY_ONLY builds no longer depend on upstream `serde`.
> Workspace manifests now alias `serde` to the facade, and `foundation_bigint`
> replaces the `num-bigint` stack inside `crypto_suite` so the guard compiles
> without crates.io big-integer code while residual `num-traits` stays in image/num-* tooling. `foundation_serialization`
> continues to expose mutually
> exclusive `serde-external`/`serde-stub` feature gates, the stub backend mirrors
> serde’s `ser`/`de` traits, visitor hierarchy, value helpers, primitive
> Serialize/Deserialize implementations, and `IntoDeserializer` adapters, and
> `cargo check -p foundation_serialization --no-default-features --features
> serde-stub` succeeds. The guard-validated inventory reflects the new crate
> boundaries.
> **Review (2025-10-12):** Testkit serial wrappers now expand without `syn`/`quote` by consuming raw token streams; the new parser still injects the `testkit::serial::lock()` guard so deterministic ordering is preserved. Foundation math tests switched to first-party floating-point helpers (`testing::assert_close[_with]`), letting the workspace drop the `approx` crate. Wallet and remote-signer builds removed the dormant `hidapi` feature flag—HID connectors remain stubbed, but FIRST_PARTY_ONLY builds no longer link native HID toolchains. Dependency inventories and the audit report were refreshed accordingly.
> **Review (2025-10-13):** Metrics aggregation now anchors on an in-house `AggregatorRecorder` that forwards every `foundation_metrics` macro emission into the existing Prometheus registry while preserving integer TLS fingerprints and runtime histograms. Monitoring utilities install a dedicated `MonitoringRecorder` so the snapshot CLI reports success/error counters through the same facade without reviving the third-party `metrics` stack.
> **Review (2025-10-12):** Replaced the storage engine’s lingering serde/foundation_sqlite shims with an in-house JSON codec and temp-file harness so manifests, WAL records, and compaction metadata now round-trip exclusively through first-party parsers, eliminating the crate’s dependency on the `foundation_serialization` facade, `serde` derives, `rusqlite`, and the `sys::tempfile` adapter in FIRST_PARTY_ONLY builds. Guarded manifests now decode via deterministic Value helpers, byte slices persist as explicit arrays, and on-disk snapshots rely on an auditable WAL/manifest representation that no longer drags third-party parsers into the first-party pipeline. The global dependency guard learned how to scope `cargo metadata` to the requesting crate’s resolved node set so FIRST_PARTY_ONLY checks flag only the offender’s transitive graph instead of every workspace package, keeping enforcement targeted while we continue retiring legacy crates. Regenerated the dependency inventory snapshot, violations report, and first-party manifest so the audit baseline reflects the storage engine’s leaner dependency DAG and the narrower guard semantics. The runtime stack simultaneously adopted the new `foundation_metrics` facade—runtime installs the recorder that feeds spawn-latency histograms and pending-task gauges into existing telemetry channels while wallet, CLI, and tooling counters now emit through first-party macros, eliminating the crates.io `metrics`/`metrics-macros` pair.

> **Review (2025-10-11):** Hardened the `http_env` TLS environment harness with a multi-sink `TLS_ENV_WARNING` registry so diagnostics, telemetry, tests, and services can all observe structured events without bespoke subscribers, expanded the HTTPS integration suite to round-trip CLI-converted identities, extended the TLS tooling surface with `contract tls stage` (`--env-file`, environment-prefix overrides, canonical export paths) so operators can fan identities out to per-service directories without bespoke scripting while retaining the prior `foundation_*` facade rollouts, shipped the `tls-manifest-guard` validator for manifest-driven reloads (now stripping optional quotes from env-file values before comparison), and wired the metrics aggregator to surface both `tls_env_warning_total{prefix,code}` and `tls_env_warning_last_seen_seconds{prefix,code}` via the shared sink. The sink now forwards BLAKE3 fingerprints through `tls_env_warning_detail_fingerprint{prefix,code}` / `tls_env_warning_variables_fingerprint{prefix,code}` and accumulates hashed occurrences on `tls_env_warning_detail_fingerprint_total{prefix,code,fingerprint}` / `tls_env_warning_variables_fingerprint_total{prefix,code,fingerprint}` so dashboards can correlate warning variants without exposing raw detail strings, while the new `tls_env_warning_detail_unique_fingerprints{prefix,code}` / `tls_env_warning_variables_unique_fingerprints{prefix,code}` gauges and accompanying `observed new tls env warning … fingerprint` info logs highlight previously unseen hashes. Warning snapshots now retain structured detail, per-fingerprint counts, rehydrate from node-exported gauges after restarts, respect the configurable `AGGREGATOR_TLS_WARNING_RETENTION_SECS` window, and remain available at `/tls/warnings/latest`, while `tls-manifest-guard` grows a `--report <path>` option that emits a machine-readable JSON summary of errors and warnings for automation hooks. `/export/all` support bundles now include `tls_warnings/latest.json` and `tls_warnings/status.json` so offline analyses carry hashed payloads and retention metadata. Follow-up work adds seven-day snapshot pruning plus a diagnostics-to-HTTP integration test, and tightens `tls-manifest-guard` with directory confinement, prefix enforcement, duplicate detection, and env-file drift warnings. The node now ships a diagnostics bridge that mirrors `TLS_ENV_WARNING` log lines into telemetry when no sinks are active (`ensure_tls_env_warning_diagnostics_bridge`) alongside a reset hook for tests, ensuring dashboards still surface structured counters during integration runs and dry deployments.
Telemetry clients can now call `tls_warning::register_tls_env_warning_telemetry_sink()` (or the re-exported `the_block::telemetry::register_tls_env_warning_telemetry_sink()`) to receive `TlsEnvWarningTelemetryEvent` callbacks (prefix, code, origin, totals, last-seen timestamp, hashed detail/variable buckets, and change flags), with guards unregistering sinks on drop so dashboards and services can rotate handlers without leaking state. Tests can clear the registry via `tls_warning::reset_tls_env_warning_telemetry_sinks_for_test()` before installing bespoke callbacks.
Telemetry follow-up the same week added a diagnostics subscriber that mirrors `TLS_ENV_WARNING` log lines into the shared sink without double-counting counters so the aggregator’s `tls_env_warning_last_seen_seconds{prefix,code}` gauge keeps advancing even when only observers fire, rewrote the aggregator’s fingerprint ingestion to decode JSON numbers and hex labels into exact 64-bit integers (eliminating the f64 rounding collisions that previously caused CLI/Prometheus mismatches), and migrated the monitoring snapshot/compare helpers to typed metric snapshots so automation consumes `MetricValue::{Float,Integer,Unsigned}` records instead of lossy `f64`s when cross-checking warning totals and per-fingerprint counters.
Fingerprint gauges now register as integer metrics to preserve the full BLAKE3 digest, and the CLI’s `contract telemetry tls-warnings` output adds an `ORIGIN` column that matches the Prometheus label set, keeping on-host drills aligned with dashboard pivots.
The latest telemetry sweep factors the fingerprint hashing into the shared `crates/tls_warning` module so every binary reuses the same BLAKE3 helpers, and the aggregator now emits `tls_env_warning_events_total{prefix,code,origin}` alongside the existing totals so dashboards can distinguish diagnostics-driven events from peer-ingested deltas while preserving hashed metadata.
Local telemetry now maintains an in-process snapshot map (`telemetry::tls_env_warning_snapshots`) so nodes, tooling, and tests can inspect totals, last deltas, detail strings, captured `variables`, fingerprint counts, and unique hash tallies without scraping metrics, and `contract telemetry tls-warnings` now includes `--probe-detail` / `--probe-variables` helpers to compute fingerprints locally before comparing them to Prometheus output while printing per-fingerprint counts to mirror the aggregator.
Grafana dashboards sprout hashed TLS fingerprint gauges, unique-fingerprint tallies, and per-fingerprint five-minute delta panels so operators can watch the `tls_env_warning_*_fingerprint` and `tls_env_warning_*_fingerprint_total` series without crafting ad-hoc queries, while Prometheus now pages on `TlsEnvWarningNewDetailFingerprint`, `TlsEnvWarningNewVariablesFingerprint`, `TlsEnvWarningDetailFingerprintFlood`, and `TlsEnvWarningVariablesFingerprintFlood` whenever brand-new hashes appear or sustained surges hit non-`none` buckets. The new `monitoring compare-tls-warnings` binary consumes `contract telemetry tls-warnings --json`, `/tls/warnings/latest`, and the `tls_env_warning_*` metrics to flag mismatched counts with a non-zero exit code, wiring deterministic automation into the TLS incident workflow.
> `/tls/warnings/status` augments the latest snapshot endpoint with retention metadata (`retention_seconds`, `active_snapshots`, `stale_snapshots`, newest/oldest timestamps), the aggregator exports matching gauges (`tls_env_warning_retention_seconds`, `tls_env_warning_active_snapshots`, `tls_env_warning_stale_snapshots`, `tls_env_warning_most_recent_last_seen_seconds`, `tls_env_warning_least_recent_last_seen_seconds`), monitoring templates render a "TLS env warnings (age seconds)" panel that graphs `clamp_min(time() - max by (prefix, code)(tls_env_warning_last_seen_seconds), 0)`, the CLI adds `contract telemetry tls-warnings` for on-host inspection alongside the combined `contract tls status` payload, and dashboards page on the new `TlsEnvWarningSnapshotsStale` alert when stale entries linger.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, codec, serialization, SQLite, TUI, TLS, and HTTP env facades are live with governance overrides enforced (2025-10-11).

This living document chronicles every deliberate shift in The‑Block's protocol economics and system-wide design. Each section explains the historical context, the exact changes made in code and governance, the expected impact on operators and users, and the trade-offs considered. Future hard forks, reward schedule adjustments, or paradigm pivots must append an entry here so auditors can trace how the chain evolved.

## Runtime WorkQueue Replacement (2025-10-12)

### Rationale

- **Remove third-party schedulers:** The in-house runtime backend still
  depended on `crossbeam-deque` for task scheduling and blocking jobs,
  blocking the dependency guard even though every other async primitive was
  first-party.
- **Unify task orchestration:** Separate injector/worker-stealer logic for the
  async executor and the blocking pool duplicated scheduling semantics and
  complicated shutdown ordering.
- **Maintain telemetry:** Runtime metrics expose
  `runtime_spawn_latency_seconds` and `runtime_pending_tasks`; the replacement
  queue had to preserve notification semantics so dashboards stayed accurate.

### Implementation Summary

- Added an `Arc`-backed `WorkQueue<T>` inside `runtime/src/inhouse/mod.rs`
  using `Mutex<VecDeque<T>>` plus a condition variable so tasks and blocking
  jobs share a first-party scheduler without lock-free dependencies.
- Rewrote the async worker loop to drain the shared queue, removed
  `crossbeam-deque`/`Injector`/`Stealer` plumbing, and tightened shutdown by
  broadcasting notifications when the runtime drops.
- Mirrored the change in the blocking pool so blocking jobs reuse the same
  queue abstraction, shrinking the concurrency surface to one implementation
  while keeping dedicated threads.
- Updated `crates/runtime/Cargo.toml` to drop the `crossbeam-deque` optional
  dependency and feature gate, regenerating the dependency inventory to show
  the slimmer graph.

### Operational Impact

- **FIRST_PARTY_ONLY builds** eliminate another third-party dependency—the
  runtime no longer relies on crossbeam while `sled` remains the only crate
  pulling it in.
- **Runtime behaviour** is unchanged from the caller’s perspective; spawn,
  cancellation, and shutdown semantics remain identical while telemetry still
  reports pending tasks and spawn latency via `foundation_metrics`.
- **Blocking jobs** continue to execute on dedicated threads, but the new
  queue lets future instrumentation (e.g., queue depth gauges) reuse the same
  abstraction.

## Aggregator & Monitoring Recorder Bridge (2025-10-13)

### Rationale

- **Align aggregation with the new facade:** Runtime and tooling now emit `foundation_metrics` macros; the aggregator still owned bespoke Prometheus handles that needed a bridge without reviving the third-party `metrics` crate.
- **Expose monitoring health through first-party counters:** Snapshot tooling lacked recorder-backed success/error tracking, forcing scripts to parse log output instead of structured metrics.

### Implementation Summary

- Added an `AggregatorRecorder` in `metrics-aggregator/src/lib.rs` that implements the first-party `Recorder` trait, fans macro events back into the Prometheus registry, preserves integer TLS fingerprint gauges, and registers the runtime spawn-latency histogram plus pending-task gauge.
- Ensured `metrics-aggregator` installs the recorder at startup (`AppState::new_with_opts` and `src/main.rs`) and extended integration coverage (`tests/telemetry.rs`) to verify macro events surface through `/metrics`.
- Introduced `monitoring/src/metrics.rs` with a lightweight `MonitoringRecorder`, recorder installation helpers, snapshot success/error counters, and unit coverage; reorganised the library/build-script split via `src/dashboard.rs` so the build pipeline reuses dashboard generation without pulling the metrics facade.
- Updated `monitoring/src/bin/snapshot.rs` to install the recorder before scraping telemetry and emit structured success/error counters through the new macros.

### Operational Impact

- **Aggregator operators** continue scraping existing endpoints while gaining first-party metrics for runtime spawn latency, pending tasks, and TLS fingerprint updates sourced through the shared recorder.
- **Monitoring automation** can alert on failed snapshot attempts using the recorder-backed counters instead of parsing stderr, and the build process remains lightweight because dashboard generation no longer includes the metrics module.
- **FIRST_PARTY_ONLY builds** keep passing guard enforcement—the new recorders rely entirely on in-house crates without reintroducing third-party telemetry dependencies.

## Post-Quantum Stub Rollout & Byte Helper Migration (2025-10-13)

### Rationale

- **Unblock FIRST_PARTY_ONLY for PQ builds:** Dilithium/Kyber experiments still depended on the crates.io `pqcrypto-*` stack, preventing guarded builds from compiling when the `quantum`/`pq` features were enabled.
- **Keep byte helper attributes first party:** Node exec/read-receipt payloads relied on the external `serde_bytes` crate to drive `#[serde(with = "serde_bytes")]` annotations; FIRST_PARTY_ONLY builds still had to link the crates.io shim.

### Implementation Summary

- Added `crates/pqcrypto_dilithium` and `crates/pqcrypto_kyber`, first-party stubs that generate deterministic keys, signatures, ciphertexts, and shared secrets using the in-house BLAKE3 facade plus OS randomness. Node commit–reveal, wallet PQ helpers, CLI identity flows, and governance tests now consume the stubs directly.
- Removed the `pqcrypto-dilithium`, `pqcrypto-internals`, `pqcrypto-traits`, and `pqcrypto-kyber` crates from workspace manifests; `Cargo.lock`, the dependency inventory, and the audit backlog now record the in-house replacements.
- Introduced `foundation_serialization::serde_bytes`, a tiny module that mirrors the external helper’s `serialize`/`deserialize` functions so existing `#[serde(with = "serde_bytes")]` annotations continue to compile without pulling in the third-party crate.
- Updated node exec/read-receipt modules to import the new helper, removed the `serde_bytes` dependency from manifests, and regenerated documentation to reflect the fully first-party path.

### Operational Impact

- **PQ feature gates** compile under FIRST_PARTY_ONLY without disabling the guard; signatures and encapsulations remain deterministic for tests while the real implementations land.
- **Wallet/CLI operators** retain the same interfaces—the stubs expose `as_bytes`/`from_bytes` helpers and deterministic encodings so existing test vectors stay stable.
- **Serde attribute usage** continues unchanged while ensuring guarded builds never reach for crates.io just to serialize byte buffers.

## Runtime Async Facade Consolidation (2025-10-12)

### Rationale

- **Eliminate duplicate async primitives:** `crates/runtime` still shipped its
  own oneshot channel and `pin-project`-derived cancelable future while the new
  `crates/foundation_async` facade already exposed the same surface.
- **Harden wake handling without third-party shims:** The interim
  `foundation_async::task::AtomicWaker` lacked deferred-wake semantics and we
  still depended on `futures::FutureExt::poll_unpin` in the in-house backend.
- **Provide test coverage for first-party async helpers:** The async facade had
  no regression tests covering `join_all`, `select2`, panic capture, or channel
  cancellation paths.

### Implementation Summary

- Replaced the runtime oneshot module with a re-export of
  `foundation_async::sync::oneshot`, removed the local file, and updated all
  call sites (runtime, node, tests) to rely on the shared facade.
- Strengthened `foundation_async::task::AtomicWaker` with a pending flag so
  wakeups triggered before registration are delivered once a waker is stored.
- Refactored `join_all` to avoid unsafe pin projection by using interior
  mutability, added deferred-output handling, and introduced a dedicated test
  suite (`crates/foundation_async/tests/futures.rs`) that exercises join
  ordering, select short-circuiting, panic trapping, and oneshot cancellation.
- Updated the in-house runtime backend to drop `poll_unpin`, use
  `Pin::new(receiver).poll(cx)` directly, and restore the internal
  `CancelOutcome` enum without relying on `pin-project` macros.

### Operational Impact

- **FIRST_PARTY_ONLY builds** now rely on a single async facade; runtime no
  longer duplicates channel code and the shared AtomicWaker survives deferred
  wakeups.
- **Runtime tests** cover the async helpers directly, catching regressions in
  join/select/oneshot behaviour without reintroducing third-party futures
  helpers.
- **Node/gateway stacks** continue to compile unchanged while the shared async
  crate accrues coverage needed for the remaining runtime primitive migrations.

## First-Party Metrics Facade Rollout (2025-10-12)

### Rationale

- **Remove external metrics crates:** The workspace still linked the crates.io `metrics`/`metrics-macros` pair for counters and histograms, blocking FIRST_PARTY_ONLY builds from passing guard enforcement.
- **Unify instrumentation:** Runtime, wallet, CLI, and tooling components needed a shared recorder surface so dashboards and aggregators ingest consistent labels without bespoke adapters.
- **Bridge existing telemetry sinks:** Node telemetry had to forward first-party events into the established Prometheus and logging pipelines without duplicating transport or recorder logic.

### Implementation Summary

- Added `crates/foundation_metrics` with first-party `counter!`, `gauge!`, and `histogram!` macros plus a recorder trait, thread-local buffer, and regression tests that cover label validation and ordering.
- Replaced `metrics` crate imports across codec/runtime/wallet/tooling with the new macros and updated manifests (`crates/codec`, `crates/runtime`, `crates/wallet`, `tools/snapshot`).
- Installed a runtime recorder in `node/src/telemetry.rs` that bridges `foundation_metrics` events into the existing Prometheus handles while emitting a runtime spawn-latency histogram and pending-task gauge.
- Regenerated dependency inventory artifacts and the first-party manifest to reflect the removal of `metrics`/`metrics-macros` from the workspace DAG.

### Operational Impact

- **FIRST_PARTY_ONLY builds** now compile without pulling the `metrics` crates, shrinking the guard violation list.
- **Operators** receive richer runtime telemetry (`runtime_spawn_latency_seconds`, `runtime_pending_tasks`) without changing scrape configuration.
- **Wallet and CLI tooling** emit counters through the same recorder surface, so downstream dashboards and support bundles ingest consistent labels without per-binary shims.

## Storage Engine JSON Codec & Guard Scope Tightening (2025-10-12)

### Rationale

- **Remove lingering third-party codecs:** The storage engine still depended on
  `foundation_serialization`, `serde`, and `sys::tempfile` for manifest/WAL
  handling, preventing `FIRST_PARTY_ONLY=1` builds from compiling the crate in
  isolation.
- **Make dependency guard diagnostics actionable:** Workspace-wide guard
  failures overwhelmed engineers when only a single crate regressed; narrowing
  the check to the requesting crate keeps enforcement focused.

### Implementation Summary

- Added `crates/storage_engine/src/json.rs` with a deterministic JSON parser,
  encoder, and typed conversion helpers plus regression tests covering unicode
  escapes, malformed numbers, trailing data, and byte-array coercions.
- Introduced `crates/storage_engine/src/tempfile.rs` with first-party temp-dir
  and named-tempfile wrappers alongside cleanup/persist tests that validate
  success and failure paths.
- Rewired the in-house and memory backends to consume the new helpers and
  dropped the `foundation_serialization`, `serde`, and `sys::tempfile`
  dependencies from `crates/storage_engine/Cargo.toml`.
- Updated `crates/dependency_guard` to resolve `cargo metadata` for the
  requesting crate only before evaluating policy violations so
  FIRST_PARTY_ONLY checks flag the offending crate instead of every workspace
  package.
- Regenerated dependency inventory artifacts to capture the slimmer storage
  engine DAG and the guard semantics change.

### Operational Impact

- **FIRST_PARTY_ONLY builds** can compile the storage engine without external
  serialization or tempfile crates, unblocking full guard enforcement.
- **Engine maintainers** have direct regression coverage for critical manifest
  and temp-file error paths, reducing the risk of malformed data making it to
  disk.
- **Engineers consuming the guard** now receive crate-scoped violation reports,
  accelerating dependency cleanup without noise from unrelated packages.

## Transport TLS Asset Wiring (2025-10-10)

### Rationale

- **Drop OpenSSL from QUIC providers:** Quinn and s2n backends relied on ad-hoc
  global state to ingest trust anchors, OCSP staples, and session caches,
  complicating the effort to ship an entirely first-party TLS stack.
- **Consistent provisioning:** Runtime selectors, tests, and operators need a
  single configuration surface to stage TLS material regardless of the active
  QUIC provider.

### Implementation Summary

- Added `transport::TlsSettings` and surfaced it on `transport::Config` so
  callers can provide optional trust anchors, OCSP staples, and TLS resumption
  caches.
- Taught the default transport factory to install (or clear) the configured
  assets when spinning up Quinn or s2n providers, ensuring stale material is
  removed when swapping providers.
- Split `s2n_backend` tests into storage and transport modules to keep targeted
  coverage without namespace collisions when building the crate in isolation.

### Operator & Developer Impact

- Nodes can stage first-party trust stores or OCSP staples without touching
  provider internals, smoothing the migration away from OpenSSL-backed tooling.
- Test harnesses can now exercise TLS asset rotation deterministically by
  injecting ephemeral stores via the shared config structure.

### Migration Notes

- Update any custom transport configuration builders to populate
  `transport::Config::tls` when providing trust anchors or resumption stores.
- Reset helpers (`reset_quinn_tls` / `reset_s2n_tls`) clear global state when a
  provider is not selected, so out-of-band installs should migrate to the shared
  configuration surface.

## First-Party Error Handling & Utility Replacements (2025-10-12)

### Rationale

- **Remove third-party facades:** The workspace still depended on `anyhow` for
  error propagation across CLI, explorer, wallet, and tooling crates. Retaining
  the external error facade blocked `FIRST_PARTY_ONLY` builds and fractured
  telemetry/observability.
- **Eliminate residual CSV/UUID crates:** The simulation harness and remote
  signer still linked crates.io implementations (`csv`, `uuid`). Replacing them
  unblocks future guard enforcement without altering downstream automation.

### Implementation Summary

- Routed all CLI/explorer/wallet/tool binaries through the existing
  `diagnostics` crate, re-exporting `anyhow!`, `bail!`, and `Context` helpers so
  ergonomics remain unchanged while the underlying error type becomes
  first-party (`diagnostics::TbError`). Added targeted `From` implementations
  for `std::io::Error`, `codec::Error`, and `foundation_sqlite::Error` so
  call-sites continue to use `?` without bespoke mapping.
- Updated `diagnostics::Result` to mirror `anyhow::Result<T, E>` semantics so
  existing signatures like `Result<T, String>` stay source-compatible.
- Replaced `tb-sim`'s dependency on the `csv` crate with a lightweight in-house
  writer that streams headers and snapshots via `std::io::Write`, preserving the
  CSV format consumed by regression dashboards.
- Generated remote signer trace IDs via a new helper that produces RFC 4122 v4
  strings from the in-house `rand` crate, removing the last `uuid` dependency.
- Added a wallet unit test that validates the trace identifier layout/uniqueness
  so log consumers can treat the string as a UUID without external helpers.
- Replaced the `xtask` lint harness dependencies (`assert_cmd`, `predicates`)
  with standard library process helpers and binary discovery, keeping the diff
  workflow while dropping third-party crates.
- Regenerated `docs/dependency_inventory.{md,json}` and
  `docs/dependency_inventory.violations.json` to remove `anyhow`, `csv`,
  `uuid`, `assert_cmd`, and `predicates` from the recorded inventory.

### Operator & Developer Impact

- Developers see the same `anyhow!`/`Context` ergonomics while telemetry and
  logging now flow through the unified diagnostics stack.
- Simulation outputs remain byte-for-byte compatible with existing dashboards
  and automation scripts.
- Remote signer traces continue to produce UUID-formatted identifiers without
  linking external crates, keeping observability and log correlation intact.

### Migration Notes

- Downstream tooling should rely on `diagnostics::anyhow::{Result, Context,
  anyhow, bail}` instead of pulling `anyhow` directly.
- CSV consumers require no changes; the new writer intentionally preserves the
  existing schema.
- Trace identifier semantics remain unchanged (lowercase hex with hyphens), so
  log processing pipelines do not need updates.

## In-House QUIC Certificates (2025-10-10)

### Rationale

- **First-party cert material:** The in-house provider previously emitted random
  byte blobs as "certificates", preventing TLS verifiers from authenticating
  peers or extracting public keys for handshake validation.
- **Key continuity:** QUIC callers need deterministic fingerprints and
  verifying keys to compare against advertised peer identities when rotating
  away from Quinn/s2n.

### Implementation Summary

- Taught `foundation_tls` how to recover Ed25519 verifying keys from DER-encoded
  certificates via `ed25519_public_key_from_der` with hardened length parsing.
- Swapped the in-house provider to generate bona fide self-signed Ed25519
  certificates using the shared rotation helpers from `foundation_tls` and
  `foundation_time`, persisting the verifying key alongside the fingerprint.
- Updated `verify_remote_certificate` to reject mismatched public keys rather
  than only hashing the payload, and threaded verifying keys through certificate
  handles so callers can enforce identity checks.
- Moved the QUIC session cache into the transport crate so
  `foundation_tls` ships as a pure certificate helper without depending on
  `rustls`. Provider adapters now manage their own caches.
- Added the `transport::cert_parser` module to replace the external
  `x509-parser` crate with a first-party DER parser for Ed25519 certificates in
  the s2n backend.
- Adjusted the test harness to provision the new TLS settings and assert the
  stricter verification path.

### Operator & Developer Impact

- QUIC integrations that opt into the in-house backend now surface real
  certificate material, enabling peer identity validation and future TLS
  handshakes without falling back to third-party providers.
- Tests and tooling can trust fingerprint comparisons to reflect the actual
  Ed25519 verifying key embedded in the certificate, closing the gap between
  the in-house backend and production Quinn/s2n deployments.

### Migration Notes

- Existing consumers should cache or distribute the verifying key exposed by
  `CertificateHandle::Inhouse` when comparing peer advertisements.
- The lossy `certificate_from_der` helper still accepts legacy blobs, but any
  handshake using invalid DER will now fail during verification; update fixtures
  accordingly.

## First-Party Transport Provider Routing (2025-10-12)

### Rationale

- **Eliminate lingering third-party QUIC fallbacks:** FIRST_PARTY_ONLY builds
  still compiled the s2n backend even when the in-house transport was selected,
  keeping the dependency audit red-lined and forcing build-guard exceptions.
- **Persist real certificate material for restarts:** The in-house store only
  tracked fingerprints and verifying keys, leaving listeners to mint fresh
  certificates on every boot and breaking fingerprint continuity.
- **Stabilise node-facing APIs:** `transport_quic` assumed s2n was always
  present, preventing FIRST_PARTY_ONLY configurations from compiling even though
  the in-house provider was feature-complete.

### Implementation Summary

- Dropped the s2n feature flag from `cfg(first_party_only)` builds so workspace
  configurations compile the transport crate with only the in-house backend.
- Expanded `transport::inhouse::CertificateStore` to serialise DER payloads
  alongside fingerprints/verifying keys, persist a sibling `.der` artefact, and
  expose install/load helpers that recreate full certificate handles without
  regenerating material.
- Added `Adapter::listen_with_certificate` so nodes can restart listeners with
  persisted certificates, keeping handshake fingerprints stable across
  rotations.
- Rebuilt `node::net::transport_quic` around a provider-agnostic facade that
  selects the active adapter at runtime, exposes a provider-aware
  `CertAdvertisement`, and falls back to the persisted in-house store when
  s2n/quinn are absent.
- Threaded provider IDs through the node bootstrap so handshake metadata and
  telemetry still carry descriptive backend labels even when the default factory
  lazily initialises the in-house provider.

### Operator & Developer Impact

- FIRST_PARTY_ONLY builds now exercise the in-house QUIC backend exclusively,
  surfacing dependency-guard violations immediately whenever third-party crates
  sneak back into the transport surface.
- Certificate fingerprints remain stable across restarts because listeners reuse
  the persisted DER material; gossip handshakes can compare advertisements
  against prior sessions instead of treating every boot as a rotation.
- Tooling that relied on `transport_quic` continues to compile with either
  provider and gains access to optional verifying-key/issued-at metadata for
  richer audit logs.

### Migration Notes

- Delete any bespoke s2n stubs wired into FIRST_PARTY_ONLY build scripts—the
  workspace now excludes the feature when that cfg is active.
- When migrating existing nodes, copy the generated `quic_certs.json` and
  adjacent `.der` artefact into the new certificate directory so fingerprints
  survive the upgrade.
- Consumers constructing `transport_quic::CertAdvertisement` manually must fill
  the new `verifying_key`/`issued_at` fields (set them to `None` if the data is
  unavailable) to match the updated struct layout.

## In-House QUIC Store Overrides & Coverage (2025-10-12)

### Rationale

- **Keep certificate stores relocatable:** Integration tests and operators need
  to stage ephemeral certificate directories without exporting
  `TB_NET_CERT_STORE_PATH` globally. The in-house adapter previously ignored the
  config-supplied cache path, so FIRST_PARTY_ONLY suites still touched the
  default home directory.
- **Detect corrupt DER assets:** Crashes or partial writes could leave zero-byte
  or malformed `.der` blobs on disk. `InhouseCertificateStore::load_certificate`
  would accept those payloads, leading to zeroed verifying keys and handshake
  mismatches at the next restart.
- **Exercise the handshake path end-to-end:** We now persist DER blobs and allow
  config-driven paths, but lacked an integration test that boots the in-house
  server, connects via the registry adapter, and proves persistence survives a
  restart.

### Implementation Summary

- Added `transport_quic::set_inhouse_cert_store_override` and wired
  `node::net::configure_transport` to respect `TransportConfig.certificate_cache`
  whenever the in-house provider is active. Non-inhouse providers clear the
  override so Quinn/s2n builds continue to rely on their own cache hooks.
- Hardened `InhouseCertificateStore::load_certificate` to delete corrupt DER
  artefacts and return `None` when the verifying key cannot be recovered,
  blocking zero-key handshakes.
- Introduced `InhouseTransportGuard` in `node/tests/net_quic.rs` that stages a
  temp certificate store, forces the override, and verifies
  `transport_quic::start_server` reuses the persisted DER across restarts while
  round-tripping payloads through the adapter.

### Operator & Developer Impact

- Tests, localnets, and operators can point the in-house transport to
  run-specific directories via `TransportConfig.certificate_cache` without
  mutating global environment variables.
- Corrupt DER files are pruned automatically, ensuring the store regenerates a
  fresh certificate instead of returning zeroed verifying keys that would break
  gossip fingerprint checks.
- The new integration coverage codifies the first-party handshake path:
  regression builds will fail if persistence breaks, the override stops working,
  or the adapter no longer echoes application data.
- Additional regression cases now pin the handshake callbacks and mixed-provider
  guard rails. `crates/transport/tests/inhouse.rs` asserts that latency,
  reuse, and failure metadata surface through the first-party callbacks, and
  `crates/transport/tests/provider_mismatch.rs` exercises Quinn ↔ in-house
  registries to ensure incompatible handles are rejected without depending on
  third-party shims.

### Migration Notes

- Update any bespoke bootstraps that relied on `TB_NET_CERT_STORE_PATH` to use
  the config-driven `certificate_cache` override for consistency with the new
  guard.
- Tests that depend on corrupt `.der` fixtures should now expect
  `load_certificate()` to return `None` and recreate the certificate; delete the
  corrupted blobs between runs to avoid persistent mismatches.

## First-Party TLS Client Adoption (2025-10-11)

### Rationale

- **Eliminate remaining OpenSSL shims:** Wallet remote signer flows still
  depended on `native-tls`, leaving `FIRST_PARTY_ONLY=1` builds blocked on
  OpenSSL and schannel while the rest of the stack moved to first-party TLS.
- **Align TLS provisioning across clients:** The HTTP client needed the same
  certificate/identity format as the in-house transport so operators and tests
  manage a single set of JSON trust anchors and identities.

### Implementation Summary

- Added `httpd::tls_client` with `TlsConnector`/`ClientTlsStream` that reuse the
  in-house handshake, X25519 key exchange, and Ed25519 certificate validation
  from `crates/httpd/src/tls.rs`.
- Migrated the wallet remote signer to the new connector, supporting optional
  client authentication, deterministic trust-anchor lookups, and in-process
  WebSocket upgrades without leaking listeners (`crates/wallet/src/remote_signer.rs`).
- Updated remote signer fixtures to JSON certificate/PKCS#8 assets and adjusted
  tests to cover mutual TLS, trust anchor rejection, and signer rotation
  (`crates/wallet/tests/remote_signer.rs`, `crates/httpd/src/tls_client.rs`).
- Added reusable HTTP client helpers for the CLI, node binaries, and the
  metrics aggregator so TLS identities and trust anchors are loaded from
  environment prefixes instead of hard-coded manifests
  (`cli/src/http_client.rs`, `node/src/http_client.rs`,
  `metrics-aggregator/src/lib.rs`, `metrics-aggregator/src/object_store.rs`).
- Refreshed the dependency snapshot and first-party audit to record the removal
  of `native-tls` and OpenSSL transitively.

### Operator & Developer Impact

- Wallet binaries, CLI tooling, node helpers, and the metrics aggregator now
  consume first-party TLS end-to-end with shared environment prefixes, enabling
  `FIRST_PARTY_ONLY=1` builds for HTTPS consumers and eliminating OpenSSL from
  remote signer deployments.

## Shared TLS Environment Helpers & Converter (2025-10-11)

### Rationale

- **Unify environment handling:** CLI, node, and tooling binaries each carried
  bespoke TLS loaders, leading to drift and inconsistent fallback behaviour.
- **Accelerate PEM migrations:** Operators needed a supported path to convert
  existing PEM material into the JSON identities consumed by the first-party
  TLS stack.

### Implementation Summary

- Added the `http_env` crate exposing `blocking_client` and `http_client`
  wrappers that delegate to `httpd` while emitting component-scoped fallbacks
  when TLS configuration is incomplete.
- Promoted sink-backed `TLS_ENV_WARNING` logging (with
  `register_tls_warning_sink`, `install_tls_warning_observer`, and
  `redirect_tls_warnings_to_stderr` helpers) so structured warnings surface via
  diagnostics, telemetry, and bespoke observers without bespoke glue in
  consumers.
- Migrated CLI, node, metrics aggregator, explorer, probe, jurisdiction, and
  example binaries to the new helpers, ensuring every HTTP consumer honours the
  same prefix-ordering semantics and logging.
- Introduced the `contract tls convert` CLI subcommand that decodes PEM or
  existing JSON identities/trust anchors and writes the canonical JSON files
  expected by the environment loaders, plus the companion `contract tls stage`
  helper that copies converted assets into per-service directories with the
  correct filename conventions, optional client-auth assets, `--env-file`
  exports, and service-specific environment-prefix overrides.
- Extended `contract tls stage` to emit per-service `tls-manifest.json` and
  `tls-manifest.yaml` files capturing staged paths, environment exports,
  renewal windows, and certificate `not_after` timestamps so orchestrators can
  audit assets before reloads; YAML output mirrors the JSON manifest for humans
  and plays nicely with config management systems.
- Installed sink-driven forwarders that increment
  `TLS_ENV_WARNING_TOTAL{prefix,code}`, stamp
  `TLS_ENV_WARNING_LAST_SEEN_SECONDS{prefix,code}`, and rehydrate warning
  snapshots from node-exported gauges after restarts via
  `install_tls_env_warning_forwarder`.
- Added local telemetry snapshots (`telemetry::tls_env_warning_snapshots()` and
  a testing reset helper) so nodes, tooling, and the new
  `contract telemetry tls-warnings` CLI path can inspect totals, last deltas,
  last-seen timestamps, structured detail, and captured environment variables
  without scraping Prometheus.
- Extended the status pipeline so the aggregator emits
  `tls_env_warning_retention_seconds`, `tls_env_warning_active_snapshots`,
  `tls_env_warning_stale_snapshots`,
  `tls_env_warning_most_recent_last_seen_seconds`, and
  `tls_env_warning_least_recent_last_seen_seconds`, ships the
  `TlsEnvWarningSnapshotsStale` alert, and exposes `contract tls status`
  (`--latest`/`--json`) so operators can fetch combined status and snapshot
  reports with remediation suggestions.
- Added integration coverage that spins up the in-house HTTPS server, verifies
  prefix preference, covers legacy fallbacks, ensures canonical environment
  exports are generated, round-trips converter output through the runtime
  loaders while emitting structured warnings for miswired variables, and now
  asserts that TLS warning logs increment the metrics counter.

### Operator & Developer Impact

- Operators configure a single set of prefixes across binaries and now receive
  machine-parseable `TLS_ENV_WARNING` lines via diagnostics plus the shared
  sink whenever identities are missing or conflicting client-auth variables are
  present. Dashboards can alert on
  `TLS_ENV_WARNING_TOTAL{prefix,code}` spikes and track
  `TLS_ENV_WARNING_LAST_SEEN_SECONDS{prefix,code}` freshness (rehydrated from
  node gauges after restarts) to spot miswired prefixes.
- PEM material can be converted into JSON identities and trust anchors without
  hand editing, then staged into service-specific directories with a single
  CLI command that also writes canonical `export FOO=` environment files and
  manifest files that systemd units consume via `TLS_MANIFEST_PATH`, reducing
  onboarding friction and reload risk.
- Regression coverage catches prefix regressions, missing identity handling,
  and converter incompatibilities before releases ship.

### Migration Notes

- Replace direct calls to `HttpClient::with_tls_from_env` or
  `BlockingClient::with_tls_from_env` with the `http_env` helpers so future
  logging/selection changes apply automatically.
- Use `contract tls convert --cert <pem> --key <pem> [--anchor <pem>]` to
  generate JSON identities and `contract tls stage --input <dir> --service
  aggregator:required=/path --service explorer=/path --env-file tls.env` to fan
  those assets out into service directories (with optional
  `label[:mode]@ENV_PREFIX` overrides); existing JSON files remain compatible
  with the new loader.
- `tls-manifest-guard` trims optional single or double quotes around env-file
  values before comparing against the manifest export, so shell-style quoting
  (`export TB_NODE_TLS_CERT="/etc/cert.pem"`) no longer causes false
  mismatches.
- Tests and local tooling use the same JSON trust anchors as the transport
  layer, reducing drift between QUIC and HTTPS provisioning.

## In-House QUIC Handshake Hardening (2025-10-10)

### Rationale

- **Reliability on first-party transport:** The initial UDP + TLS adapter sent
  a single `ClientHello` and trusted best-effort delivery, making in-house QUIC
  materially flakier than the Quinn and s2n providers it is intended to
  replace.
- **Peer identity continuity:** Certificate advertisements only persisted
  fingerprints, forcing higher-level identity checks to guess at the verifying
  key when rotating or auditing peer material.

### Implementation Summary

- Added an exponential retransmission schedule inside the client handshake so
  `ClientHello` frames are resent within the configured timeout and verified by
  an updated server loop that replays cached `ServerHello` payloads and rejects
  stale entries after 30 s.
- Extended the handshake table with explicit TTL tracking, duplicate handling,
  and unit tests that cover cached replies, expiration, and retransmission
  bounds.
- Persisted the Ed25519 verifying key alongside the fingerprint in the JSON
  advertisement store, regenerating cache entries that predate the new schema
  so peers always publish verifiable material.
- Tightened integration coverage to exercise the upgraded handshake path and
  the richer advertisement metadata.

### Operator & Developer Impact

- In-house QUIC connections now enjoy the same retry guarantees as the
  third-party backends, reducing the gap while FIRST_PARTY_ONLY builds phase in
  the native transport.
- Certificate rotation feeds both fingerprints and verifying keys through the
  on-disk advertisement cache, simplifying peer validation across node, CLI,
  and explorer surfaces.

### Migration Notes

- Older advertisement files lacking a verifying key are automatically
  regenerated on load, but operators should verify that distribution pipelines
  and dashboards consume the new field when auditing peer identity.
- The new retransmission schedule honours the existing handshake timeout; tune
  `handshake_timeout` in `config/quic.toml` if deployments relied on the
  previous best-effort behaviour.

## First-Party SQLite Facade (2025-10-10)

### Rationale

- **Dependency sovereignty:** Explorer, CLI, and log/indexer tooling depended
  directly on `rusqlite`, preventing `FIRST_PARTY_ONLY=1` builds from compiling
  and complicating efforts to stub or replace the backend.
- **Unified ergonomics:** Ad-hoc parameter macros and per-tool helpers made it
  easy to drift between positional vs. named parameters and inconsistent error
  handling; a shared facade normalises values, parameters, and optional
  backends.

### Implementation Summary

- Added `crates/foundation_sqlite` exporting `Connection`, `Statement`, `Row`,
  `params!`/`params_from_iter!`, and a lightweight `Value` enum covering the
  rusqlite types we use today.
- Default builds enable the `rusqlite-backend` feature, delegating to the
  existing engine while unifying parameter conversion and query helpers.
- Introduced a first-party `FromValue` decoding trait plus
  `ValueConversionError`, letting the facade translate rows without depending on
  `rusqlite::types::FromSql` so stub and future native engines share identical
  call sites.
- `foundation_sqlite` exposes a stub backend when the feature is disabled,
  returning `backend_unavailable` errors so `FIRST_PARTY_ONLY=1 cargo check`
  surfaces missing implementations without pulling third-party code.
- Migrated explorer query helpers, the CLI `logs` command, and the
  indexer/log-indexer tooling to call the facade (including the new
  `query_map` collector) instead of `rusqlite` directly.

### Operator & Developer Impact

- Tooling builds continue to work with SQLite when the default feature is
  enabled, while first-party-only builds now fail fast with clear errors rather
  than missing symbols.
- Shared parameter/value handling removes subtle differences between tools,
  making it easier to audit SQL statements and extend coverage.
- Future work can swap the backend (or add an embedded engine) inside
  `foundation_sqlite` without touching downstream crates.

### Migration Notes

- Downstream tooling must depend on `foundation_sqlite` instead of `rusqlite`.
- Keep the `rusqlite-backend` feature enabled for production until the in-house
  engine lands; tests can exercise the stub by setting `FIRST_PARTY_ONLY=1` or
  disabling the feature.
- Follow-up work will replace the stub with a native engine so
  `FIRST_PARTY_ONLY=1` builds succeed end-to-end.

## First-Party Log Store (2025-10-14)

### Rationale

- **SQLite retirement:** The log indexer and CLI now default to the
  sled-backed `log_index::LogStore`, removing the final runtime dependency on
  SQLite while preserving optional migrations for historical archives.
- **Shared ingestion/search code:** Tooling previously duplicated ingestion,
  encryption, and pagination logic. Consolidating the flow into a single crate
  ensures CLI, node, explorer, and monitoring surfaces stay in lockstep while
  telemetry hooks track ingestion outcomes per correlation ID.

### Implementation Summary

- Added `crates/log_index` with sled-backed storage, ingest/seek helpers,
  optional encryption, legacy SQLite migration shims (behind the
  `sqlite-migration` feature), and rotate-key utilities.
- Rebuilt `tools/log_indexer.rs`, `contract logs`, and the node/explorer
  integrations on top of the crate, wiring observer callbacks into telemetry
  counters and normalising environment fallbacks (`--db`, `TB_LOG_STORE_PATH`,
  `TB_LOG_DB_PATH`).
- Extended unit tests to cover plaintext, encrypted, and rotate-key flows while
  skipping automatically when the `foundation_serde` stub backend is active.

### Operator & Developer Impact

- Default builds ship a fully first-party log store with key rotation and
  telemetry hooks. Operators only enable the `sqlite-migration` feature when
  importing legacy databases.
- Tooling picks up consistent command/help text and environment overrides,
  simplifying runbook updates and preventing drift between CLI and REST flows.
- The shared crate exposes a stable API for future dashboards or automation to
  embed log ingestion/search without re-implementing storage glue.

## Foundation Time Facade (2025-10-10)

### Rationale

- **Timestamp determinism:** Storage repair logging, S3 snapshot signing, and
  transport certificate rotation all encoded ad-hoc `time` crate calls with
  inconsistent formatting and error handling.
- **First-party builds:** The direct dependency on the upstream `time` crate
  blocked `FIRST_PARTY_ONLY=1` builds and complicated efforts to audit
  formatting changes across runtime and tooling.

### Implementation Summary

- Added `crates/foundation_time` with an in-house `UtcDateTime`/`Duration`
  implementation, deterministic calendar math, and helpers to emit ISO-8601 and
  AWS-style compact timestamps.
- Replaced metrics aggregator S3 signing, storage repair file naming, and the
  QUIC certificate generator with the new facade so runtime/tooling code no
  longer imports `time` directly.
- Landed the first-party `foundation_tls` certificate builder so QUIC rotation
  and test tooling construct Ed25519 X.509 certificates without `rcgen`, using
  facade validity windows end-to-end.

### Operator & Developer Impact

- Timestamp formatting is now consistent across runtime and tooling, reducing
  the odds of drift between logging, signing, and certificate validity windows.
- `FIRST_PARTY_ONLY` builds can compile these surfaces without linking the
  upstream crate; the QUIC stack now signs certificates exclusively through the
  foundation TLS facade.
- Future features (e.g., governance snapshot formatting or log timestamp
  normalization) can extend the facade without reintroducing external
  dependencies.

## Foundation TUI Facade (2025-10-10)

### Rationale

- **Dependency parity:** The node's networking CLI relied on the third-party
  `colored` crate for ANSI output, blocking `FIRST_PARTY_ONLY=1` builds and
  preventing consistent colour policies across tooling.
- **Operator control:** Colour output needed to respect environment overrides
  (`NO_COLOR`, `CLICOLOR`) and TTY detection without depending on crates we aim
  to remove from production builds.

### Implementation Summary

- Added `crates/foundation_tui` with ANSI colour helpers, a `Colorize` trait,
  and environment-aware detection that honours `TB_COLOR`, `NO_COLOR`, and
  terminal detection via `sys::tty`.
- Swapped the node networking CLI (`node/src/bin/net.rs`) to call the new
  helpers, removing the direct `colored` dependency from the workspace.
- Extended `sys::tty` with `stdout_is_terminal`/`stderr_is_terminal`/`stdin_is_terminal`
  helpers so other tooling can reuse the detection logic without reimplementing
  platform-specific checks.

### Operator & Developer Impact

- CLI colour output is now consistent across platforms, respects operator
  overrides, and no longer depends on crates.io packages.
- `FIRST_PARTY_ONLY` builds compile without the `colored` crate while keeping
  familiar `line.red()` ergonomics for downstream tooling.
- Additional styling (bold, underline, background colours) can be added to the
  facade without reintroducing external dependencies.

## Foundation Unicode Normalizer (2025-10-10)

### Rationale

- **First-party input hygiene:** Handle registration, DID validation, and CLI
  helpers previously called into ICU via `icu_normalizer`, blocking
  `FIRST_PARTY_ONLY=1` builds and inflating the dependency surface with large
  Unicode data tables.
- **Deterministic behaviour:** The team needs a predictable, auditable
  normalizer with a clear ASCII fast-path so operator tooling and governance
  flows can agree on canonical forms without relying on opaque upstream tables.

### Implementation Summary

- Introduced `crates/foundation_unicode` with an `nfkc` normalizer that
  short-circuits ASCII inputs, provides compatibility mappings for common
  compatibility characters, and exposes accuracy flags for non-ASCII fallbacks.
- Swapped the node handle registry and integration tests to the facade,
  removing the `icu_normalizer` and `icu_normalizer_data` crates from the
  workspace.
- Documented the facade in the dependency audit so future Unicode work can
  extend the mapping tables without reintroducing third-party code.

### Operator & Developer Impact

- Handle normalisation is now controlled entirely by first-party code; future
  tweaks can land alongside governance decisions instead of waiting on ICU
  updates.
- `FIRST_PARTY_ONLY` builds link the light-weight facade instead of the ICU
  ecosystem, dramatically shrinking the dependency tree for identity tooling.
- The accuracy flag allows downstream callers to detect when non-ASCII fallback
  mappings are used and add additional validation as needed.

## Xtask Git Diff Rewrite (2025-10-10)

### Rationale

- **Remove libgit2 stack:** The `xtask summary` helper depended on the
  third-party `git2` bindings which pulled in libgit2, `url`, `idna`, and the
  ICU normalization crates even after the runtime/CLI migrations, keeping
  `FIRST_PARTY_ONLY=1` builds from linking cleanly.
- **Stabilise tooling behaviour:** Shelling out to the git CLI mirrors the
  commands operators and CI already run, avoids binding-specific corner cases,
  and dramatically shrinks the transitive dependency graph for release checks.

### Implementation Summary

- Replaced the libgit2-backed diff logic with thin wrappers around `git
  rev-parse` and `git diff --patch`, preserving the JSON summary output while
  leaning on the existing CLI.
- Dropped the `git2` dependency from `tools/xtask`, allowing the workspace to
  remove the `url`/`idna_adapter`/ICU stack that the bindings required.
- Updated the first-party manifest and dependency snapshot so `FIRST_PARTY_ONLY`
  guards now pass without whitelisting libgit2.

### Operator & Developer Impact

- Developer tooling no longer links against libgit2, closing another gap on the
  path to all-first-party builds.
- CI summary jobs use the same git CLI output developers see locally, reducing
  surprises when reviewers validate PR summaries or dependency guard output.

## Wrapper Telemetry Integration (2025-09-25)

### Rationale

- **Unified observability:** Runtime, transport, overlay, storage engine, coding, codec, and crypto backends now surface consistent gauges/counters so operators can correlate incidents with dependency switches without grepping logs.
- **Governance evidence:** Backend selections and dependency policy violations are visible to voters before approving rollouts or escalations.
- **CLI and automation:** On-call engineers can fetch the exact wrapper mix from production fleets via a single CLI command.

### Implementation Summary

- Extended `node/src/telemetry.rs` with per-wrapper gauges (`runtime_backend_info`, `transport_provider_connect_total{provider}`, `overlay_backend_active`, `storage_engine_backend_info`, `coding_backend_info`, `codec_serialize_fail_total{profile}`, `crypto_suite_signature_fail_total{backend}`) plus size/failure histograms where applicable.
- Added wrapper snapshots to `metrics-aggregator`, exposing a REST `/wrappers` endpoint, schema docs in `monitoring/metrics.json`, and Grafana dashboards that chart backend selections, failure rates, and policy violation gauges across operator/dev/telemetry views.
- Landed a `contract-cli system dependencies` subcommand that queries the aggregator and formats wrapper status (provider name, version, commit hash, policy tier) for on-call debugging and change management.
- Wired the dependency registry tooling to emit a runtime telemetry `dependency_policy_violation` gauge, enabling alerts when policy drift appears.

### Operator & Governance Impact

- Dashboards and CLI output include provider/codec/crypto labels, making phased rollouts auditable during change windows.
- Governance proposals gain concrete evidence before ratifying backend swaps or remediating policy drift; registry snapshots remain part of release artifacts.
- Runbooks now reference wrapper metrics when diagnosing network incidents or signing off on dependency simulations.

### Next Steps

- Extend storage migration tooling so RocksDB↔sled transitions can be rehearsed alongside wrapper telemetry.
- Feed wrapper summaries into the planned dependency fault simulation harness to rehearse provider outages under controlled chaos scenarios.

## Dependency Sovereignty Pivot (2025-09-23)

### Rationale

- **Risk management:** The node relied on 800+ third-party crates spanning runtime,
  transport, storage, coding, crypto, and serialization. A surprise upstream
  change could invalidate safety guarantees or stall releases.
- **Operational control:** Wrapping these surfaces in first-party crates lets
  governance gate backend selection, run fault drills, and schedule replacements
  without pleading for upstream releases.
- **Observability & trust:** Telemetry, CLI, and RPC endpoints now report active
  providers (runtime, transport, overlay, storage engine, codec, crypto) so
  operators can audit rollouts and correlate incidents with backend switches.

### Implementation Summary

- Formalised the pivot plan in [`docs/pivot_dependency_strategy.md`](pivot_dependency_strategy.md)
  with 20 tracked phases covering registry, tooling, wrappers, governance,
  telemetry, and simulation milestones.
- Delivered the dependency registry, CI gating, runtime wrapper, runtime
  adoption linting, QUIC transport abstraction, provider introspection,
  release-time vendor syncs, and documentation updates as completed phases.
- Inserted a uniform review banner across the documentation set referencing the
  pivot date so future audits can confirm alignment.

### Operator & Governance Impact

- Operators must reference wrapper crates rather than upstream APIs and consult
  the registry before accepting dependency changes.
- Governance proposals will inherit new parameter families to approve backend
  selections; telemetry dashboards already surface the metadata required to
  evaluate those votes.
- Release managers must include registry snapshots and vendor tree hashes with
  every tagged build; CI now fails if policy drift is detected.

### What’s Next

- Ship the overlay, storage-engine, coding, crypto, and codec abstractions,
  then extend telemetry/governance hooks across each wrapper.
- Build the dependency fault simulation harness so fallbacks can be rehearsed in
  staging before enabling on production.
- Migrate wallet, explorer, and mobile clients onto the new abstractions as they
  land, keeping documentation in sync with the pivot guide.

## QUIC Transport Abstraction (2025-09-23)

### Rationale for the Trait Layer

- **Provider neutrality:** The node previously called Quinn APIs directly and carried an optional s2n path. Abstracting both behind `crates/transport` lets governance swap providers or inject mocks without forking networking code.
- **Deterministic testing:** Integration suites can now supply in-memory providers implementing the shared traits, delivering deterministic handshake behaviour for fuzzers and chaos harnesses.
- **Telemetry parity:** Handshake callbacks, latency metrics, and certificate rotation counters now originate from a common interface so dashboards remain consistent regardless of backend.

### Implementation Summary

- Introduced `crates/transport` with `QuicListener`, `QuicConnector`, and `CertificateStore` traits, plus capability enums consumed by the handshake layer.
- Moved Quinn logic into `crates/transport/src/quinn_backend.rs` with pooled connections, retry helpers, and replaceable telemetry callbacks.
- Ported the s2n implementation into `crates/transport/src/s2n_backend.rs`, wrapping builders in `Arc`, sharing certificate caches, and exposing provider IDs.
- Added a `ProviderRegistry` that selects backends from `config/quic.toml`, surfaces provider metadata to `p2p::handshake`, and emits `quic_provider_connect_total{provider}` telemetry.
- Updated CLI/RPC surfaces to display provider identifiers, rotation timestamps, and fingerprint history sourced from the shared certificate store.

### Operator Impact

- Configuration lives in `config/quic.toml`; reloads rebuild providers without restarting the node.
- Certificate caches are partitioned by provider so migration between Quinn and s2n retains history.
- Telemetry dashboards can segment connection successes/failures by provider, highlighting regressions during phased rollouts.

### Testing & Tooling

- `node/tests/net_quic.rs` exercises both providers via parameterised harnesses, while mocks cover retry loops.
- CLI commands (`blockctl net quic history`, `blockctl net quic stats`, `blockctl net quic rotate`) expose provider metadata for on-call triage.
- Chaos suites reuse the same trait interfaces, ensuring packet-loss drills and fuzz targets remain backend agnostic.

## CT Subsidy Unification (2024)

The network now mints every work-based reward directly in CT. Early devnets experimented with an auxiliary reimbursement ledger, but governance retired that approach in favour of a single, auditable subsidy store that spans storage, read delivery, and compute throughput.

### Rationale for the Switch
- **Operational Simplicity:** A unified CT ledger eliminates balance juggling, decay curves, and swap mechanics.
- **Transparent Accounting:** Subsidy flows reconcile with standard wallets, easing audits and financial reporting.
- **Predictable UX:** Users can provision gateways or upload content with a plain CT wallet—no staging balances or side ledgers.
- **Direct Slashing:** Burning CT on faults or policy violations instantly reduces circulating supply without custom settlement paths.

### Implementation Summary
- Removed the auxiliary reimbursement plumbing and its RPC surfaces, consolidating rewards into the CT subsidy store.
- Introduced global subsidy multipliers `beta`, `gamma`, `kappa`, and `lambda` for storage, read delivery, CPU, and bytes out. These values live in governance parameters and can be hot-tuned.
- Added a rent-escrow mechanism: every stored byte locks `rent_rate_ct_per_byte` CT, refunding 90 % on deletion or expiry while burning 10 % as wear-and-tear.
- Reworked coinbase generation so each block mints `STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` alongside the decaying base reward.
- Redirected the former reimbursement penalty paths to explicit CT burns, ensuring punitive actions reduce circulating supply.

Changes shipped behind feature flags with migration scripts (such as `scripts/purge_legacy_ledger.sh` and updated genesis templates) so operators could replay devnet ledgers and confirm balances and stake weights matched across the switch. Historical blocks remain valid; the new fields simply appear as zero before activation.

### Impact on Operators
- Rewards arrive entirely in liquid CT.
- Subsidy income depends on verifiable work: bytes stored, bytes served with `ReadAck`, and measured compute. Stake bonds still back service roles, and slashing burns CT directly from provider balances.
- Monitoring requires watching `subsidy_bytes_total{type}`, `subsidy_cpu_ms_total`, and rent-escrow gauges. Operators should also track `inflation.params` to observe multiplier retunes.

Archive `governance/history` to maintain a local audit trail of multiplier votes and kill-switch activations. During the first epoch after upgrade, double-check that telemetry exposes the new subsidy and rent-escrow metrics; a missing gauge usually indicates lingering legacy configuration files or dashboard panels.

### Impact on Users
- Uploads, hosting, and dynamic requests work with standard CT wallets. No staging balances or alternate instruments are required.
- Reads remain free; the cost is socialized via block-level inflation rather than per-request fees. Users only see standard rate limits if they abuse the service.

Wallet interfaces display the refundable rent deposit when uploading data and automatically return 90 % on deletion, making the lifecycle visible to non-technical users.

### Governance and Telemetry
Governance manages the subsidy dial through `inflation.params`, which exposes the five parameters:
```
 beta_storage_sub_ct
 gamma_read_sub_ct
 kappa_cpu_sub_ct
 lambda_bytes_out_sub_ct
 rent_rate_ct_per_byte
```
An accompanying emergency knob `kill_switch_subsidy_reduction` can downscale all subsidies by a voted percentage. Every retune or kill‑switch activation must append an entry to `governance/history` and emits telemetry events for on-chain tracing.

The kill switch follows a 12‑hour timelock once activated, giving operators a grace window to adjust expectations. Telemetry labels multiplier changes with `reason="retune"` or `reason="kill_switch"` so dashboards can plot long-term trends and correlate them with network incidents.

### Reward Formula Reference
The subsidy multipliers are recomputed each epoch using the canonical formula:
```
multiplier_x = (ϕ_x · I_target · S / 365) / (U_x / epoch_seconds)
```
where `S` is circulating CT supply, `I_target` is the annual inflation ceiling (currently 2 %), `ϕ_x` is the inflation share allocated to class `x`, and `U_x` is last epoch's utilization metric. Each multiplier is clamped to ±15 % of its prior value, doubling only if `U_x` was effectively zero to avoid divide-by-zero blow-ups. This dynamic retuning ensures inflation stays within bounds while rewards scale with real work.

### Pros and Cons
| Aspect | Legacy Reimbursement Ledger | Unified CT Subsidy Model |
|-------|-----------------------------|--------------------------|
| Operator payouts | Separate balance with bespoke decay | Liquid CT every block |
| UX for new users | Required staging an auxiliary balance | Wallet works immediately |
| Governance surface | Multiple mint/decay levers | Simple multiplier votes |
| Economic transparency | Harder to audit total issuance | Inflation capped ≤2 % with public multipliers |
| Regulatory posture | Additional instrument to justify | Single-token utility system with CT sub-ledgers |

### Migration Notes
Devnet operators should run `scripts/purge_legacy_ledger.sh` to wipe obsolete reimbursement data and regenerate genesis files without the legacy balance field. Faucet scripts now dispense CT. Operators must verify `inflation.params` after upgrade and ensure no deprecated configuration keys persist in configs or dashboards.

### Future Entries
Subsequent economic shifts—such as changing the rent refund ratio, altering subsidy shares, or introducing new service roles—must document their motivation, implementation, and impact in a new dated section below. This file serves as the canonical audit log for all system-wide model changes.

## Durable Compute Settlement Ledger (2025-09-21)

### Rationale for Persistence & Dual-Ledger Accounting

- **Crash resilience:** The in-memory compute settlement shim dropped balances on restart. Persisting CT flows (with legacy industrial columns retained for tooling) in RocksDB guarantees recovery, even if the node or process exits unexpectedly.
- **Anchored accountability:** Governance required an auditable trail that explorers, operators, and regulators can replay. Recording sequences, timestamps, and anchors ensures receipts reconcile with the global ledger.
- **Ledger clarity:** Providers and buyers need to understand CT balances after every job. Persisting the ledger avoids race conditions when reconstructing balances from mempool traces and keeps the legacy industrial column available for regression tooling.

### Implementation Summary

- `Settlement::init` now opens (or creates) `compute_settlement.db` inside the configured settlement directory, wiring sled-style helpers that load or default each sub-tree (`ledger_ct`, `ledger_it`, `metadata`, `audit_log`, `recent_roots`, `next_seq`). Test builds without the legacy `storage-rocksdb` toggle transparently fall back to an ephemeral directory while production deployments rely on the in-house engine.
- Every accrual, refund, or penalty updates both the in-memory ledger and the persisted state via `persist_all`, bumping a monotonic sequence and recomputing the Merkle root (`compute_root`).
- `Settlement::shutdown` always calls `persist_all` on the active state and flushes RocksDB handles before dropping them, ensuring integration harnesses (and crash recovery drills) see fully durable CT balances (with zeroed industrial fields) even if the node exits between accruals.
- `Settlement::submit_anchor` hashes submitted receipts, records the anchor in `metadata.last_anchor_hex`, pushes a marker into the audit deque, and appends a JSON line to the on-disk audit log through `state::append_audit`.
- Activation metadata (`metadata.armed_requested_height`, `metadata.armed_delay`, `metadata.last_cancel_reason`) captures the reason for every transition between `DryRun`, `Armed`, and `Real` modes. `Settlement::arm`, `cancel_arm`, and `back_to_dry_run` persist these fields immediately and emit telemetry via `SETTLE_MODE_CHANGE_TOTAL{state}`.
- Telemetry counters `SETTLE_APPLIED_TOTAL`, `SETTLE_FAILED_TOTAL{reason}`, `SLASHING_BURN_CT_TOTAL`, and `COMPUTE_SLA_VIOLATIONS_TOTAL{provider}` expose a live view of accruals, refunds, and penalties. Dashboards can alert on stalled anchors or repeated SLA violations.
- RPC endpoints `compute_market.provider_balances`, `compute_market.audit`, and `compute_market.recent_roots` serialize the persisted data so the CLI and explorer can render provider balances, audit trails, and continuity proofs. Integration coverage lives in `node/tests/compute_settlement.rs`, `cli/tests/compute.rs`, and `explorer/tests/compute_settlement.rs`.

### Operational Impact

- **Operators** should monitor the new RPCs and runtime telemetry counters to ensure balances drift as expected, anchors land on schedule, and SLA burns are visible. Automate backups of `compute_settlement.db` alongside other state directories.
- **Explorers and auditors** can subscribe to the audit feed, correlate sequence numbers with Merkle roots, and flag any divergence between local mirrors and the node-provided anchors.
- **Governance and finance** teams gain deterministic evidence of CT burns, refunds, and payouts, unblocking treasury reconciliation and upcoming SLA enforcement proposals.

### Migration Notes

- Nodes upgrading from the in-memory shim should point `settlement_dir` (or the default data directory) at persistent storage before enabling `Real` mode. The first startup migrates balances into RocksDB with a zeroed sequence.
- Automation that previously scraped in-process metrics must switch to the RPC surfaces described above. CLI invocations use the default build; enable the optional `sqlite-migration` feature only when importing legacy SQLite snapshots before returning to the minimal configuration.
- Backups should include `compute_settlement.db` and the `audit.log` file written by `state::append_audit` so post-incident reviews retain both ledger state and anchor evidence.
## Unicode Handle Telemetry and CLI Surfacing (2025-10-10)

### Rationale

- **Dependency sovereignty:** Identity normalization now runs entirely through
  the first-party `foundation_unicode` facade. Latin-1 and Greek letters map to
  ASCII fallbacks so operators no longer depend on ICU tables to register common
  names.
- **Operational visibility:** Registrations now emit
  `identity_handle_normalization_total{accuracy}` so clusters can quantify how
  many handles relied on approximate transliteration and adjust onboarding flows
  accordingly.
- **Operator tooling:** The CLI gained `contract identity register|resolve|normalize`
  commands that show both local and remote normalization results, ensuring human
  operators can detect mismatches before the registry persists a handle.

### Implementation Summary

- Added transliteration tables for accented Latin-1 and Greek characters to the
  Unicode facade. The registry records `NormalizationAccuracy` alongside each
  registration and propagates it through the RPC layer.
- Instrumented the registry with the `identity_handle_normalization_total`
  counter and surfaced accuracy in the RPC response schema.
- Built a dedicated CLI identity module that displays accuracy labels and warns
  when the node accepted an approximate normalization.
- Extended the CLI `identity register` subcommand with an optional
  `--pq-pubkey` flag gated behind the `pq-crypto`/`quantum` features so Dilithium
  registrants can forward their post-quantum key material alongside Ed25519
  payloads.

### Operational Impact

- **Operators** should watch the new counter for spikes in approximate
  normalizations and coach users toward handles that normalize exactly.
- **Support tooling** can invoke the CLI’s `identity normalize` command to audit
  handles offline and reproduce the registry’s transliteration decisions.

## Deterministic TLS Rotation Plans (2025-10-10)

### Rationale

- **Pre-computable rotations:** QUIC and s2n listeners now schedule certificates
  via a deterministic `RotationPolicy`, allowing rotation daemons to prepare
  leaf certificates in advance without relying on randomness.
- **Chain issuance:** The transport layer can bind QUIC listeners with complete
  CA chains, unblocking deployments that terminate on intermediate CAs or need
  to present both leaf and issuer certificates during handshake.
- **Interoperability tests:** Integration suites verify both Quinn and s2n
  providers against CA-signed paths using the new builder, guarding against
  regressions as we extend the TLS facade.

### Implementation Summary

- Introduced `RotationPolicy`/`RotationPlan` in `foundation_tls` and wired the
  certificate builders to derive validity windows and serial numbers from a
  deterministic schedule.
- Updated QUIC and s2n backends to consume rotation plans instead of random
  serials and to expose helpers for installing certificate chains.
- Added CA-signed integration tests for both providers and exposed a listener
  helper (`listen_with_chain`) so nodes can bind endpoints with full chains.
- Implemented provider-specific `ListenerHandle::as_*`/`into_*` helpers for
  Quinn, s2n-quic, and in-house backends, and taught `listen_with_chain` to
  borrow certificate slices, eliminating unnecessary vector clones when
  installing large chains in tests or runtime wiring.

### Operational Impact

- **Rotation jobs** can reuse the shared policy to stage certificates ahead of
  time and coordinate rollouts across multiple nodes.
- **Deployments** issuing certificates from internal CAs can feed chain
  artifacts directly into the QUIC adapter without patching the transport
  crate.

## In-house QUIC Handshake Skeleton (2025-10-10)

### Rationale

- **Dependency sovereignty:** Replaces the placeholder `inhouse_backend` with
  a first-party UDP + TLS handshake so transport builds no longer rely on
  Quinn/s2n just to smoke-test the in-house provider.
- **Certificate fidelity:** Shares the `foundation_tls` certificate helpers so
  the in-house backend generates/verifies the same Ed25519 material used by
  external providers, keeping CLI/RPC validation paths consistent.
- **Stateful advertising:** Persists listener advertisements and rotation data
  through a dedicated certificate store, allowing nodes to reload fingerprints
  without piping through third-party JSON codecs.

### Implementation Summary

- Introduced `crates/transport/src/inhouse/` with `adapter.rs`,
  `messages.rs`, `certificate.rs`, and `store.rs` implementing the UDP
  handshake, message encoding, certificate generation, and JSON-backed
  advertisement persistence.
- Updated the transport registry (`crates/transport/src/lib.rs`) to load the
  new module, pass handshake timeouts through `Config`, and surface
  provider-specific helpers via `ListenerHandle::as_inhouse`.
- Replaced the legacy integration tests with
  `crates/transport/tests/inhouse.rs`, covering successful round trips,
  certificate mismatches, metadata introspection, and certificate-store
  rotation.

### Operational Impact

- **Operators** can now exercise the in-house transport end-to-end without
  enabling third-party QUIC crates, paving the way for
  `FIRST_PARTY_ONLY=1` transport builds.
- **Certificate tooling** can rely on the shared store to inspect fingerprints
  and issued-at timestamps when debugging node rotations.
- **Telemetry** continues to surface handshake success/failure via the
  existing callbacks, ensuring dashboards reflect the in-house backend just
  like Quinn and s2n.

## Windows IOCP Reactor Migration (2025-10-14)

### Rationale

- **Remove the Windows 64-handle ceiling:** The previous WSA event loop capped
  the runtime at 64 descriptors per shard, forcing higher-level code to split
  registrations and juggle per-event wakers. Migrating to IOCP batching removes
  this constraint so sockets, timers, and custom wake-ups share a single
  completion queue.
- **Align wake semantics across platforms:** Posting wakers through
  `PostQueuedCompletionStatus` mirrors the epoll/kqueue integration, keeping the
  runtime’s readiness model consistent and avoiding bespoke Windows-only wake
  paths.
- **Eliminate third-party Windows bindings:** Implementing the necessary Win32
  types and FFI in-house lets us drop the lingering `windows_sys` dependency and
  keep FIRST_PARTY_ONLY builds clean.

### Implementation Summary

- Replaced the WSA event loop with an IOCP-backed reactor that associates every
  socket with a completion port and drains readiness through
  `GetQueuedCompletionStatusEx`, including waker triggers posted from the
  runtime.
- Sharded the legacy WSA waiters behind lightweight threads that translate WSA
  signals into IOCP completions so existing registration paths retain their
  semantics while scaling past the old 64-handle cap.
- Implemented `AsRawSocket` for the first-party TCP/UDP wrappers and updated the
  runtime reactor to treat raw handles generically (`ReactorRaw`) across Unix and
  Windows.
- Added a Windows-specific stress test (`crates/sys/tests/reactor_windows_scaling.rs`)
  that registers 96 UDP sockets to ensure the IOCP backend can scale under
  FIRST_PARTY_ONLY builds.
- Wired `FIRST_PARTY_ONLY=1 cargo check --target x86_64-pc-windows-gnu` into CI
  and the `just check-windows` recipe so cross-target regressions surface without
  reintroducing third-party shims.
- Temporarily routed the runtime’s Windows file watcher through the existing
  polling fallback, gating the unfinished native watcher behind a
  `windows-fs-watcher` feature until an IOCP directory change loop ships.

### Operational Impact

- **Windows hosts** can now register as many sockets as needed without fragmenting
  watchers, and waker triggers behave identically to Unix, simplifying support
  and observability.
- **CI coverage** exercises FIRST_PARTY_ONLY Windows builds alongside Linux,
  ensuring future regressions are caught before release.
- **Documentation and tooling** point operators at the new `just check-windows`
  recipe while noting that Windows file watching currently uses the polling
  stub, keeping expectations clear until the native watcher lands.

## Dependency Registry Check Telemetry (2025-10-14)

### Rationale

- **All-or-nothing checks:** Prior runs reported drift only as a boolean,
  leaving operators blind once the CLI bailed. Persisting the full diff and
  telemetry snapshot keeps automation informed even when the command aborts.
- **Structured diagnostics:** Enumerating additions, removals, policy diffs, and
  root-package churn clarifies what changed so follow-up triage doesn’t require
  manual diff tooling.

### Implementation Summary

- Added a `check` module that compares baseline/generated comparison keys and
  records additions, removals, field-level updates, policy changes, and root
  package churn.
- Updated `runner::execute` to emit detailed drift narratives, surface policy
  violation counts, and persist outcomes through
  `output::write_check_telemetry` before returning.
- Introduced `dependency-check.telemetry` with a status gauge and per-kind
  counters so CI/alerting can watch `status="drift"` and the associated counts.
- Extended the CLI integration suite with a failing-baseline test that asserts
  the narrative, telemetry labels, and metrics payload alongside existing
  artifact checks.
- Seeded an additional metadata fixture covering cfg-targeted dependencies and
  `workspace_default_members` fallbacks to ensure depth calculations remain
  accurate across platform-specific graphs.

### Operational Impact

- **CI/alerting** can now scrape `dependency-check.telemetry` to detect drift or
  policy violations without rerunning the CLI.
- **Operator workflows** receive actionable messages listing the exact crates and
  policies that changed, reducing mean time to remediation.
- **Future migrations** can rely on the telemetry snapshot to compare drift over
  time, even when check mode exits early.

## Default Transport Provider Switch (2025-10-10)

### Rationale

- **First-party by default:** With the in-house UDP/TLS adapter now carrying
  parity coverage, the transport configuration should prefer it automatically
  so new nodes no longer reach for Quinn before custom code.
- **Build readiness:** Bundling the `inhouse` feature into the node’s
  transport dependency keeps the provider available in every QUIC-enabled
  build while the third-party stacks remain compiled for comparison tests.

### Implementation Summary

- Updated `transport::Config::default` to resolve the preferred provider at
  compile time, prioritising the in-house backend when it is enabled.
- Enabled the `inhouse` feature on the node’s transport dependency so QUIC
  builds ship the first-party implementation alongside legacy providers.

### Operational Impact

- **Node boot defaults** now point at the in-house provider whenever it is
  compiled, reducing manual configuration for operators embracing
  `FIRST_PARTY_ONLY` builds.
- **CI configurations** maintain Quinn and s2n support for parity suites, but
  the runtime registry starts with the custom adapter, accelerating
  first-party rollout.
- **FIRST_PARTY_ONLY gating** now routes the node’s transport dependency
  through target-specific sections in `Cargo.toml`, disabling the Quinn feature
  and relying on the session cache stub when the guard is active so transport
  builds no longer pull `rustls` into first-party-only checks.
