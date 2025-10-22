# Status & Roadmap
> **Review (2025-10-22, mid-morning+):** The CLI wallet suite now snapshots the
signer metadata array end-to-end. `fee_floor_warning` asserts the metadata vector
for ready and override previews, and a dedicated `wallet_signer_metadata` module
covers local, ephemeral, and session signers while checking the auto-bump
telemetry event—all via first-party `JsonMap` builders—so FIRST_PARTY_ONLY runs
no longer rely on mock RPC servers to validate the JSON surface.
> **Review (2025-10-22, early morning):** Wallet previews now expose signer
metadata via `BuildTxReport::signer_metadata`, and the CLI suite asserts on the
JSON emitted for ready, needs-confirmation, ephemeral, and session flows—snapshotting
the metadata array to keep FIRST_PARTY_ONLY runs hermetic. Service-badge and telemetry commands gained
helper-backed unit tests that snapshot the JSON-RPC envelopes for
`service_badge.verify`/`issue`/`revoke` and `telemetry.configure`, eliminating
mock servers and serde conversions while exercising the shared builders. The
mobile push notification and node difficulty examples mirror production by
replacing their last `foundation_serialization::json!` literals with explicit
`JsonMap` assembly, so documentation tooling stays aligned with the first-party
JSON pipeline.
> **Review (2025-10-21, mid-morning):** Contract CLI modules now route JSON
> construction through a shared `json_helpers` module that exposes string/number
> constructors and JSON-RPC envelope helpers. Compute, service-badge, scheduler,
> telemetry, identity, config, bridge, and TLS commands compose payloads via
> explicit `JsonMap` builders, governance listings serialize through a typed
> wrapper, and the node runtime log sink plus staking/escrow wallet binary reuse
> the same helpers. This removes the final `foundation_serialization::json!`
> macros from operator tooling while keeping legacy wire shapes intact and
> FIRST_PARTY_ONLY runs deterministic.
> **Review (2025-10-20, late evening):** Canonical transaction helpers now run
> entirely on the cursor facade. Node `canonical_payload_bytes`, CLI signing,
> and the Python bindings forward to `encode_raw_payload`, signed-transaction
> hashing reuses the manual writer, and decode helpers call
> `decode_raw_payload`, eliminating the `foundation_serde` stub path that
> previously tripped the base-fee regression under FIRST_PARTY_ONLY.
> **Review (2025-10-20, afternoon++):** Compute-market RPC responses now use the
> shared JSON helper to render capability snapshots, utilization maps, and audit
> history without touching `json::to_value`, while DEX escrow status/release
> payloads hand-assemble payment proofs and Merkle roots so the entire surface
> stays on the first-party facade. Peer metrics builders gained deterministic
> ordering plus unit coverage, tightening the JSON refactor before the remaining
> responders migrate.
> **Review (2025-10-20, morning):** Ledger and mempool persistence now rely on
> the first-party cursor stack end to end. `MempoolEntryDisk` records
> `serialized_size`, the startup rebuild path consumes that cache before
> re-encoding, and new regression tests cover the legacy decode helpers so
> archived snapshots load without `binary_codec`. This keeps RPC snapshot
> exporters, CLI tooling, and FIRST_PARTY_ONLY builds on in-house serialization
> without regressions.
> **Review (2025-10-14, endgame++):** Net and gateway fuzz harnesses now reuse
> the shared `foundation_fuzz` modules, removing `libfuzzer-sys`/`arbitrary`
> while smoke tests exercise the entry points directly. `foundation_serde` and
> `foundation_qrcode` permanently dropped their external-backend toggles so the
> remote signer, CLI, and tooling all build on stubbed first-party code. With the
> last optional crates.io hooks gone, the workspace lockfile resolves solely to
> in-house crates and FIRST_PARTY_ONLY runs cover every target.
> **Review (2025-10-14, closing push+++):** RPC fuzz harnesses now spin up
> per-test identity state via `sys::tempfile`, eliminating shared sled
> directories while the new smoke suite calls the in-house `run_request`
> dispatcher directly. The sled legacy importer’s builder powers the migration
> path and ships regression tests that reopen flushed manifests across multiple
> trees, and the legacy manifest CLI now enforces deterministic CF ordering plus
> default-column emission through first-party integration tests. Together these
> close the remaining fuzz/legacy-tooling gaps for FIRST_PARTY_ONLY runs.
> **Review (2025-10-14, near midnight++):** Jurisdiction policy packs now rely
> on handwritten JSON conversions and `diagnostics::log` instead of serde + the
> third-party `log` crate. The crate exposes `PolicyPack::from_json_value`,
> `from_json_slice`, and matching `SignedPack` helpers so RPC, CLI, and
> governance modules can manipulate raw JSON without external codecs while
> FIRST_PARTY_ONLY builds stay green. Fresh unit tests cover signature array/
> base64 decoding and malformed pack rejection, and the dependency inventory
> reflects the removed `log` edge.
> **Review (2025-10-14, late evening+++):** Dependency governance artifacts now
> include machine-readable summaries and dashboard hooks. The CLI runner emits
> `dependency-check.summary.json`, CI preflights (`tools/xtask`) print the parsed
> verdict, release provenance hashes the summary alongside telemetry/metrics, and
> monitoring dashboards/alerts now visualise drift, policy status, and snapshot
> freshness. Integration/release tests enforce the new artefacts so automation
> remains deterministic.
> **Review (2025-10-14, midday++):** Registry check mode now publishes drift
> telemetry and granular diagnostics. The CLI’s failure path stages additions,
> removals, field updates, policy diffs, and root-package churn before writing
> `dependency-check.telemetry`, so automation can trip alerts on
> `status="drift"` or per-kind gauges without re-running the command. Integration
> coverage drives a failing baseline to assert the narrative and metrics output,
> and new metadata fixtures capture cfg-targeted dependencies plus
> `workspace_default_members` fallbacks to keep depth calculations honest across
> platform-specific workspaces.
> **Review (2025-10-14, pre-dawn++):** Dependency governance automation gained a
> reusable CLI runner that writes registry JSON, violations, telemetry manifests,
> and optional snapshots in one pass, surfaces `RunArtifacts` for automation, and
> honours a `TB_DEPENDENCY_REGISTRY_DOC_PATH` override so tests don’t touch the
> committed inventory. A new end-to-end CLI test drives that runner against the
> fixture workspace, asserting on JSON payloads, telemetry counters, snapshot
> emission, and manifest contents. Parser coverage now includes a complex
> metadata fixture with optional/git/duplicate edges to harden adjacency
> deduplication and origin detection, and log rotation writes gained a rollback
> guard that restores the original ciphertext if any sled insert fails mid-run.
> **Review (2025-10-14, late night+):** Dependency registry policy loading and
> snapshotting now run entirely on the serialization facade. TOML configs parse
> through the new `foundation_serialization::toml::parse_table` helper, tiers/
> licenses/settings normalise manually, and JSON registries use handwritten
> `Value` conversions plus `json::to_vec_value` so serde drops from the crate.
> Unit/integration suites execute under the stub backend without skips, and the
> new TOML regression test keeps the low-level parser audited.
> **Review (2025-10-14, late night):** Log archiving now rotates encryption keys
> atomically—the CLI stages every entry before writing, adds regression coverage
> for the failure path, and the JSON availability probe exercises a full
> `LogEntry` round-trip so FIRST_PARTY_ONLY builds skip cleanly when the stub
> facade is active. The dependency registry CLI invokes `cargo metadata`
> directly and parses the graph through the first-party JSON facade, dropping
> the crates.io `cargo_metadata`/`camino` pair while expanding unit and
> integration coverage to detect stub backends automatically.
> **Review (2025-10-14, afternoon):** TLS automation is now backed by fully
> first-party serialization. The `foundation_serde` stub grew option/sequence/
> map/tuple/array coverage, `foundation_serialization::json::Value` regained
> manual serde parity, and the CLI’s TLS status/snapshot/certificate structs
> now implement handwritten serializers/deserializers so we can drop derives
> entirely. `contract tls status --json` and the TLS conversion/staging flows
> round-trip on the stub backend, and `FIRST_PARTY_ONLY=0 cargo test -p
> contract-cli --lib` passes end-to-end. Node cleanup landed alongside the
> serialization work: aggregator/quic configs call the shared default helpers,
> storage engine selection reuses `default_engine_kind()`, peer reputations seed
> timers via the shared `instant_now()` guard, compute offers expose an
> `effective_reputation_multiplier()` helper for telemetry and price board
> recording, and the pipeline binary codec now validates field counts through
> the cursor helpers so the overflow guard is exercised. The workspace builds
> without lingering node warnings, keeping guard runs focused on real gaps while
> the TLS automation shipped earlier in the week stays intact. Fresh regression
> coverage now locks the paths in place: `cli/src/tls.rs` ships JSON round-trip
> tests for warning status/snapshot payloads (including optional-field elision
> and unknown-field tolerance), `crates/foundation_serialization/tests/json_value.rs`
> verifies the manual `Value` codec rejects non-finite floats and preserves
> nested objects, and `node/src/storage/pipeline/binary.rs` exercises the field
> count guard via `write_field_count_rejects_overflow` so the encoder can’t
> regress silently.
> **Review (2025-10-14, mid-morning):** Terminal prompting is now fully
> first-party and covered by regression tests. `sys::tty` exposes a reusable
> helper that toggles echo suppression and trims newlines, `foundation_tui::prompt`
> adds override hooks for scripted inputs, and the CLI log commands gained unit
> tests for optional/required passphrase flows. FIRST_PARTY_ONLY builds keep
> interactive commands intact while coverage guards regressions.
> **Review (2025-10-14):** Cross-platform networking is now anchored on first-party
> code across Linux, BSD/macOS, and Windows. `crates/sys/src/reactor/platform_windows.rs`
> now drives an IOCP-backed backend that associates sockets with a completion
> port, translates WSA events into queued completions, and posts runtime wakers
> via `PostQueuedCompletionStatus`, eliminating the prior 64-handle ceiling.
> `crates/sys/src/net/windows.rs` mirrors the Unix socket constructors with
> `WSASocketW`, implements `AsRawSocket`, and keeps FIRST_PARTY_ONLY builds free
> of `socket2`/`mio` on Windows. Runtime file watchers now reuse the same stack:
> Linux/BSD modules ride the `sys::inotify`/`sys::kqueue` shims, and Windows
> consumes the IOCP-backed `DirectoryChangeDriver` (`crates/sys/src/fs/windows.rs`)
> with explicit `Send` guarantees and the new `foundation_windows` bindings in
> `crates/sys/Cargo.toml`. Regression coverage adds
> `crates/sys/tests/reactor_windows_scaling.rs` alongside the UDP stress harness
> (`crates/sys/tests/net_udp_stress.rs`) and existing TCP suites to guard
> readiness semantics and ordering, and `FIRST_PARTY_ONLY=1 cargo check --target
> x86_64-pc-windows-gnu` now passes for both `sys` and `runtime`. Remaining
> `tokio`-driven `mio` edges stay tracked for retirement alongside the planned
> Windows watcher integration tests.
> **Review (2025-10-12):** Runtime now schedules async tasks and blocking jobs via a shared first-party `WorkQueue`, removing the `crossbeam-deque`/`crossbeam-epoch` dependency pair while preserving spawn latency and pending task gauges. `foundation_bigint` now ships deterministic arithmetic/parsing/modpow regression tests that lock the first-party engine against the historical vectors exercised by the previous external crate.
> **Review (2025-10-11):** Hardened the `http_env` helper crate so every CLI/node/aggregator/explorer binary shares one TLS loader with sink-backed warnings and observer hooks, shipped the `contract tls convert` and enhanced `contract tls stage` commands (canonical `--env-file` exports, service prefix overrides, PEM chain resilience, manifest generation with renewal reminders) for PEM→JSON conversion and asset fan-out, introduced the `tls-manifest-guard` CLI/systemd helper so reloads validate manifests, environment exports, and renewal windows before touching the binaries (now tolerating optional quotes in env files), migrated the remaining HTTP clients onto the new helpers, wired the aggregator to ingest node-side `tls_env_warning_total{prefix,code}` deltas while stamping `tls_env_warning_last_seen_seconds{prefix,code}` via the shared sink (rehydrated from node gauges and bounded by `AGGREGATOR_TLS_WARNING_RETENTION_SECS`), and added BLAKE3 fingerprint gauges/counters (`tls_env_warning_detail_fingerprint{prefix,code}`, `tls_env_warning_variables_fingerprint{prefix,code}`, `tls_env_warning_detail_fingerprint_total{prefix,code,fingerprint}`, `tls_env_warning_variables_fingerprint_total{prefix,code,fingerprint}`) alongside the unique-fingerprint gauges (`tls_env_warning_detail_unique_fingerprints{prefix,code}`, `tls_env_warning_variables_unique_fingerprints{prefix,code}`) and first-seen fingerprint logs so dashboards can correlate hashed warning variants without raw detail strings and operators can flag novel hashes. Integration tests spin up the in-house HTTPS server to verify prefix selection, legacy fallbacks, converter round-trips, and the telemetry path, the Python dashboard helper was replaced with the first-party monitoring snapshot binary while binary codec consolidation continues across node, crypto suite, telemetry, and harness tooling, the latest warning payloads persist at `/tls/warnings/latest`, node telemetry retains local warning snapshots (now with per-fingerprint counts and unique tallies), `/export/all` bundles `tls_warnings/latest.json`/`status.json` for offline review, and `contract telemetry tls-warnings` gained JSON/label filters plus per-fingerprint totals and `--probe-detail` / `--probe-variables` helpers alongside `tls-manifest-guard --report` for orchestration pipelines. Subsequent hardening prunes `/tls/warnings/latest` snapshots after seven days, exercises the sink→HTTP ingestion path end to end, and extends `tls-manifest-guard` with directory confinement, prefix enforcement, and env-file drift warnings. The shared `crates/tls_warning` helpers now back every fingerprint calculation and the aggregator exposes `tls_env_warning_events_total{prefix,code,origin}` so dashboards can distinguish diagnostics observers from peer-ingest deltas without duplicating hashing logic. Fingerprint gauges now surface as integer metrics so 64-bit BLAKE3 digests survive ingestion intact, and `contract telemetry tls-warnings` prints an `ORIGIN` column that mirrors the Prometheus label set for incident playbooks.
> `telemetry::ensure_tls_env_warning_diagnostics_bridge()` now mirrors warning log
> lines into metrics whenever no sinks are configured (offline tooling, focused
> tests), and `reset_tls_env_warning_forwarder_for_testing()` lets harnesses swap
> sinks or exercise diagnostics-only scenarios without leaking global state.
Grafana templates now render hashed fingerprint, unique-fingerprint, and five-minute delta panels so rotations can monitor the `tls_env_warning_*_fingerprint`/`tls_env_warning_*_fingerprint_total` series directly, Prometheus gained `TlsEnvWarningNewDetailFingerprint`, `TlsEnvWarningNewVariablesFingerprint`, `TlsEnvWarningDetailFingerprintFlood`, and `TlsEnvWarningVariablesFingerprintFlood` alerts to escalate previously unseen hashes or sustained surges, and the new `monitoring compare-tls-warnings` helper verifies `contract telemetry tls-warnings --json` against `/tls/warnings/latest` plus the Prometheus series to flag drift with a machine-friendly exit code.
> `/tls/warnings/status` now surfaces retention health (window, active count, stale snapshots, and last-seen bounds), the aggregator exports the matching gauges (`tls_env_warning_retention_seconds`, `tls_env_warning_active_snapshots`, `tls_env_warning_stale_snapshots`, `tls_env_warning_most_recent_last_seen_seconds`, `tls_env_warning_least_recent_last_seen_seconds`), Grafana templates add a "TLS env warnings (age seconds)" panel so operators can audit stale prefixes directly from dashboards when widening the retention window or rotating identities, monitoring ships the `TlsEnvWarningSnapshotsStale` alert, `contract telemetry tls-warnings` surfaces local node snapshots with JSON/label filters, and `contract tls status` continues to produce human-readable or JSON reports with remediation hints.
> Dependency pivot status: Runtime (now free of `crossbeam-deque`), transport, overlay, storage_engine, coding, crypto_suite, codec, serialization, SQLite, TUI, TLS, and HTTP env facades are live with governance overrides enforced (2025-10-12).

Mainnet readiness: 98.3/100 · Vision completion: 93.3/100.
The runtime-backed HTTP client and TCP/UDP reactor now power the node and CLI stacks, and the aggregator, gateway, explorer, and indexer surfaces all serve via the in-house `httpd` router. Tracking that migration, alongside the TLS layer, keeps the dependency-sovereignty
pivot and wrapper rollout plan are central to every
milestone; see [`docs/pivot_dependency_strategy.md`](pivot_dependency_strategy.md)
for the canonical phase breakdown referenced by subsystem guides.
Known focus areas: harden the dependency guard by keeping CI and `tools/xtask`
blocking on the new first-party inventory, publish dashboard alerts for drift, and
document the runbook for downstream teams consuming the in-house crates. Expand
coverage around treasury disbursement visuals in explorer dashboards, integrate
compute-market SLA metrics with automated alerting, extend bridge docs with
multisig signer-set walkthroughs plus release-verifier guides, add end-to-end
coverage for the DEX cursor codecs (CLI/explorer flows, escrow regression
fuzzing), stand up the dependency fault simulation harness, finish the multisig
wallet UX polish, and harden the Dilithium/Kyber stubs with production-ready
test vectors and telemetry hooks. Remote-signer already ships on the
`foundation_qrcode` facade; remaining platform work focuses on rolling the
`foundation_windows` bindings through ancillary tooling so operators inherit the
same first-party APIs as the core node.

### Tooling migrations

- Explorer now binds through the first-party `httpd` stack with optional TLS and
  mutual-auth support, enabling downstream crates to exercise handlers via the
  in-process request builder (`explorer/src/main.rs`, `explorer/src/lib.rs`).
- The indexer CLI has moved from Clap/Axum to `cli_core` plus `httpd`, reusing
  the shared router helpers and optional TLS wiring for the serve subcommand
  (`tools/indexer/src/main.rs`, `tools/indexer/src/lib.rs`).
- Governance, ledger, metrics-aggregator, overlay peer stores, node telemetry,
  and crypto helpers now rely on the `foundation_serialization` facade
  (JSON/binary/base58); remaining serde_json/bincode usage is isolated to
  auxiliary tooling tracked in `docs/pivot_dependency_strategy.md`.
- `tools/dependency_registry` now parses policy TOML via
  `foundation_serialization::toml::parse_table`, maps structs to manual JSON
  `Value`s, and emits artifacts with `json::to_vec_value`, dropping serde while
  keeping stub-mode tests always-on with new regression fixtures.
- Gateway read receipts now encode/decode via `foundation_serialization::binary_cursor`
  helpers, removing the serde derive in that module while keeping the legacy
  CBOR fallback alive for historical receipts and establishing the cursor API
  for upcoming migrations.
- Storage rent escrow, manifests, provider profiles, and repair failure
  records now persist exclusively through cursor helpers
  (`node/src/storage/{fs.rs,manifest_binary.rs,pipeline/binary.rs,repair.rs}`),
  with regression tests locking legacy bytes, large manifests, redundancy
  variants, legacy payloads that lacked the modern optional fields, and a
  new randomized property harness plus sparse-manifest repair integration test
  keeping parity with the retired binary shim.
- `crates/testkit_macros` now expands serial test wrappers without the
  `syn`/`quote`/`proc-macro2` stack, keeping the shared serial guard in-house.
- `foundation_math` test suites rely on first-party floating-point assertion
  helpers, removing the external `approx` dependency.
- Wallet binaries and the remote-signer CLI removed the dormant `hidapi`
  feature flag; HID connectors remain stubbed but no longer pull native
  toolchains into FIRST_PARTY_ONLY builds.
- Runtime’s async facade now routes through `crates/foundation_async`:
  `join_all`/`select2`/oneshot re-export from the shared crate, the first-party
  `AtomicWaker` delivers deferred wakeups, and coverage in
  `crates/foundation_async/tests/futures.rs` exercises join ordering, select
  short-circuiting, panic capture, and cancellation paths. The legacy runtime
  oneshot module has been removed.
- DEX order books, trade logs, AMM pools, and escrow snapshots now persist via
  first-party cursor helpers (`node/src/dex/{storage.rs,storage_binary.rs}`),
  dropping the `binary_codec` shim while regression fixtures and randomized
  parity tests (`order_book_matches_legacy`, `trade_log_matches_legacy`,
  `escrow_state_matches_legacy`, `pool_matches_legacy`) lock the legacy sled
  bytes. Follow-up: extend CLI/explorer integration tests to exercise the new
  codecs end to end and capture escrow snapshot fuzzers.
- Gossip message envelopes, raw transactions, blob transactions, and full
  blocks now serialize via dedicated cursor helpers
  (`node/src/net/message.rs`, `node/src/transaction/binary.rs`,
  `node/src/block_binary.rs`) with quantum/non-quantum parity fixtures and a
  comprehensive payload test suite that exercises handshake, peer set, drop
  map, blob chunk, block/chain broadcast, and reputation variants. DEX/storage
  manifest regressions now inspect cursor output directly instead of
  round-tripping through `binary_codec`, completing the removal of the shim
  from networking and ledger persistence.
- Identity DID and handle registries now persist through
  `identity::{did_binary,handle_binary}` with cursor helpers and compatibility
  suites covering remote attestations, pq-key toggles, and truncated payloads,
  clearing the last sled-backed `binary_codec` usage in identity while CLI and
  explorer flows retain facade derives for JSON exports; seeded property suites
  plus the `identity_snapshot` integration test hammer randomized payloads and
  mixed legacy/current sled dumps to guard migrations.
- Proof-rebate tracker persistence moved onto the cursor helpers and the shared
  `util::binary_struct` routines, removing `binary_codec`/serde from
  `node/src/light_client/proof_tracker.rs` while compatibility tests assert the
  legacy 8-byte fallback.
- Explorer, CLI, node, and monitoring tooling now share the sled-backed
  `log_index` crate for ingestion, search, and key rotation. The optional
  `sqlite-migration` feature only gates legacy imports via the
  `foundation_sqlite` facade, so default builds drop direct SQLite usage while
  retaining compatibility with archived `.db` snapshots. The facade now loads
  and saves through the in-house JSON helpers (`database_to_json` /
  `database_from_json`), and the focused test suite locks conflict resolution,
  ORDER/LIMIT evaluation, LIKE predicates, and provider join emulation to the
  first-party engine.
- Metrics aggregator timestamp signing, storage repair logging, and QUIC
  certificate rotation now depend on the `foundation_time` facade, centralising
  formatting and removing direct `time` imports ahead of the native certificate
  builder. QUIC and s2n listeners now draw deterministic validity windows and
  serial numbers from `foundation_tls::RotationPolicy`, and the transport
  adapter can bind listeners with complete CA chains.
- Wallet remote signer flows, the CLI RPC client, node HTTP helpers, and the
  metrics aggregator now use the first-party `httpd::TlsConnector` with
  environment-driven trust anchor/identity loading, eliminating the
  `native-tls` shim and unblocking `FIRST_PARTY_ONLY=1` builds for HTTPS
  consumers across tooling.
- The network CLI now renders colours through the `foundation_tui` facade,
  dropping the third-party `colored` crate while keeping ANSI output gated on
  terminal detection and operator overrides.
- The contract CLI gained identity subcommands that reuse the
  `foundation_unicode` facade, display normalization accuracy, and warn when a
  handle required transliteration so operators can intervene before
  registration.
- A workspace-local `rand` crate and stubbed `rand_core` now back all
  randomness helpers. The crate exposes deterministic `fill`, `choose[_mut]`, and
  slice sampling APIs with dedicated coverage (`crates/rand/tests/seq.rs`) plus
  rejection-sampling range helpers (`crates/rand/tests/range.rs`) so large
  domains avoid modulo bias. The coding fountain harness runs entirely on the
  first-party RNG with new parity-budget and burst-loss regression tests, and
  simulation tooling (`sim/did.rs`) consumes the helpers so account rotation
  never falls back to crates.io RNGs. `tools/xtask` enforces `FIRST_PARTY_ONLY`
  on dependency audits now that the `--allow-third-party` escape hatch is gone.
- CLI, light-client, and transport path discovery flow through the new
  `sys::paths` adapters, removing the legacy `dirs` dependency and aligning
  migration scripts with the first-party OS abstraction.
- `http_env` wraps both blocking and async HTTP clients in a shared environment
  loader with component-tagged fallbacks, sink-backed warnings, and observer
  hooks; the TLS env integration tests exercise multi-prefix selection,
  missing-identity error reporting, canonical `--env-file` exports, and
  service-prefix overrides, keeping the new helpers `FIRST_PARTY_ONLY`
  friendly.
Downstream tooling now targets the shared
`governance` crate, compute settlement and the matcher enforce per-lane fairness
with staged seeding, fairness deadlines, starvation warnings, and per-lane
telemetry, the mobile gateway cache persists ChaCha20-Poly1305–encrypted
responses with TTL min-heap sweeping, restart replay, and operator controls,
wallet binaries propagate signer sets and telemetry, the transport registry now
abstracts Quinn and s2n providers behind `crates/transport` while surfacing
provider metadata to CLI/RPC consumers, the codec crate unifies serde/bincode/CBOR
usage with telemetry hooks, the crypto suite fronts signatures/hashing/KDF/SNARK
helpers, and the RPC client keeps bounded retries through clamped fault rates and
saturated exponential backoff.

The auxiliary reimbursement ledger has been fully retired. Every block now mints `STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` in the coinbase, with epoch‑retuned `beta/gamma/kappa/lambda` multipliers smoothing inflation to ≤ 2 %/year. Fleet-wide peer metrics feed a dedicated `metrics-aggregator`, the scheduler supports graceful `compute.job_cancel` rollbacks, fee-floor policy changes persist into `GovStore` history with rollback hooks and telemetry, and DID anchors flow through explorer APIs for cross-navigation with wallet addresses. Historical context and migration notes are in [`docs/system_changes.md`](system_changes.md#ct-subsidy-unification-2024).

## Economic Model Snapshot

Every subsidy bucket follows a one‑dial multiplier formula driven by realised
utilisation:

\[
\text{multiplier}_x = \frac{\phi_x I_{\text{target}} S / 365}{U_x / \text{epoch\_secs}}
\]

Adjustments clamp to ±15 % of the previous value; if usage `U_x` approaches
zero, the last multiplier doubles to keep incentives alive. Base miner rewards
shrink with the effective miner count via a logistic curve

\[
R_0(N) = \frac{R_{\max}}{1 + e^{\xi (N - N^\star)}}
\]

with hysteresis `ΔN ≈ √N*` to damp flash joins and leaves. Full derivations and
worked examples live in [`docs/economics.md`](economics.md).

For a subsystem-by-subsystem breakdown with evidence and remaining gaps, see
[docs/progress.md](progress.md).

## Strategic Pillars

| Pillar | % Complete | Highlights | Gaps |
| --- | --- | --- | --- |
| **Governance & Subsidy Economy** | **96.4 %** | Inflation governors tune β/γ/κ/λ multipliers and rent rate; multi-signature release approvals, attested fetch/install tooling, fee-floor policy timelines, durable proof-rebate receipts, and DID revocation history are archived in `GovStore` alongside CLI telemetry with rollback support. The shared `governance` crate exports first-party sled persistence, proposal DAG validation, and Kalman helpers for all downstream tooling. | Wire treasury disbursement timelines into explorer dashboards and publish dependency metadata before opening external submissions. |
| **Consensus & Core Execution** | 93.6 % | Stake-weighted leader rotation, deterministic tie-breaks, multi-window Kalman difficulty retune, release rollback helpers, coinbase rebate integration, and the parallel executor guard against replay collisions. | Formal proofs still absent. |
| **Smart-Contract VM & UTXO/PoW** | 87.5 % | Persistent contract store, deterministic WASM runtime with debugger, and EIP-1559-style fee tracker with BLAKE3 PoW headers. | Opcode library parity and formal VM spec outstanding. |
| **Storage & Free-Read Hosting** | **93.8 %** | Receipt-only logging, hourly batching, L1 anchoring, `gateway.reads_since` analytics, crash-safe `SimpleDb` snapshot rewrites, a unified `storage_engine` crate that abstracts RocksDB/sled/memory providers, the shared `coding` crate with XOR parity and RLE compression fallbacks behind audited rollout policy plus telemetry/bench-harness validation, first-party sled codecs with randomized property suites and sparse-manifest repair integration coverage, and a ChaCha20-Poly1305–encrypted mobile cache with TTL min-heap sweeping, restart replay, entry/queue guardrails, CLI/RPC observability, and invalidation hooks keep reads free yet auditable and durable across restarts. | Incentive-backed DHT storage and offline reconciliation remain prototypes. |
| **Networking & Gossip** | 98.3 % | QUIC mutual-TLS rotation with diagnostics/chaos harnesses, cluster `metrics-aggregator`, partition watch with gossip markers, LRU-backed deduplication with adaptive fanout, shard-affinity persistence, CLI/RPC metrics via `net.peer_stats`/`net gossip-status`, and a selectable `p2p_overlay` backend with libp2p/stub implementations plus telemetry gauges. Gateway REST, metrics-aggregator HTTP, explorer, and CLI tooling now run on the shared `httpd` router, eliminating the `hyper`/`axum` stack from production and test harnesses. Peer metrics sled snapshots persist through `peer_metrics_binary`, keeping persistence entirely on the first-party binary cursor while retaining compatibility coverage; gossip wire payloads now encode/decode via `node/src/p2p/wire_binary.rs` and the new `net::message` helpers so serde/bincode are no longer required on message envelopes; and runtime file watching now leans on the first-party `sys::inotify`/`sys::kqueue` wrappers instead of `nix`. | Large-scale WAN chaos tests outstanding; long-lived overlay soak tests and dependency registry crypto/coding wrappers still open. |
| **Compute Marketplace & CBM** | 95.8 % | Capability-aware scheduler weights offers by reputation, lane-aware matching enforces per-`FeeLane` batching with fairness windows and deadlines, starvation detection, staged seeding, batch throttling, and persisted lane-tagged receipts, settlement tracks CT balances with activation metadata, and telemetry/CLI/RPC surfaces expose queue depths, wait ages, latency histograms, and fee floors. | Finish wiring SLA telemetry into the foundation dashboard alerts and surface automated resolutions in explorer timelines. |
| **Trust Lines & DEX** | 87.2 % | Authorization-aware trust lines, cost-based multi-hop routing, slippage-checked order books, and on-ledger escrow with partial-payment proofs. Telemetry gauges `dex_escrow_locked`/`dex_escrow_pending`/`dex_escrow_total` track utilisation (total aggregates all escrowed funds). Persistence now runs on first-party codecs in `node/src/dex/{storage.rs,storage_binary.rs}`, removing the legacy `binary_codec` shim while regression suites lock legacy bytes and `EscrowSnapshot` documents sled tables. | Cross-chain settlement proofs and advanced routing features outstanding. |
| **Cross-Chain Bridges** | 89.4 % | Per-asset channel persistence via `SimpleDb`, multi-signature relayer quorums, governance-controlled incentive parameters (`BridgeIncentiveParameters`), sled-backed duty/accounting ledgers surfaced through `bridge.relayer_accounting`/`bridge.duty_log` and `blockctl bridge accounting/duties`, telemetry for challenges/slashes (`BRIDGE_CHALLENGES_TOTAL`, `BRIDGE_SLASHES_TOTAL`), reward claims, settlement submissions, and dispute outcomes (`BRIDGE_REWARD_CLAIMS_TOTAL`, `BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL`, `BRIDGE_SETTLEMENT_RESULTS_TOTAL{result,reason}`, `BRIDGE_DISPUTE_OUTCOMES_TOTAL{kind,outcome}`), cursor/limit pagination with `next_cursor` responses across `bridge.reward_claims`/`bridge.settlement_log`/`bridge.dispute_audit`, a trait-backed CLI transport (`BridgeRpcTransport`) with in-memory mocks replacing the HTTP harness in `cli/tests`, dedicated CLI coverage for `BridgeCmd::DisputeAudit`, Grafana bridge panels charting five-minute deltas for the new counters, and partition-aware deposits with refreshed integration coverage (`node/tests/bridge.rs`, `node/tests/bridge_incentives.rs`). | Multi-asset wrapping, external settlement proofs, reward-claim plumbing for accrued incentives, and long-horizon dispute audits remain. |
| **Wallets, Light Clients & KYC** | 96.9 % | CLI and hardware wallet support, remote signer workflows, mobile light-client SDKs, session-key delegation, auto-update orchestration, fee-floor caching with localized warnings/JSON output, telemetry-backed QoS overrides, and pluggable KYC hooks. Wallets now consume the shared crypto suite’s first-party Ed25519 backend, propagate escrow hash algorithms and multisig signer sets, export remote signer metrics, integrate platform-specific device probes with telemetry/overrides/log uploads through the new first-party Android/iOS helpers, surface rebate history/leaderboards across CLI and explorer, and lock light-client persistence behind deterministic fixtures that exercise `FIRST_PARTY_ONLY` guard parity and compressed snapshot recovery. | Polish multisig UX, harden production mobile distributions, and document signer-history exports. |
| **Monitoring, Debugging & Profiling** | 95.8 % | First-party dashboards rendered from `runtime::telemetry` snapshots, metrics-to-logs correlation with automated QUIC dumps, VM trace counters, DID anchor gauges, per-lane `matches_total`/`match_loop_latency_seconds` charts, mobile cache gauges (`mobile_cache_*`, `mobile_tx_queue_depth`), the `the_block_light_client_device_status{field,freshness}` gauge, and CLI debugger/profiling utilities ship with nodes; wallet QoS events and fee-floor rollbacks now plot alongside DID timelines, bridge/gossip dashboards ingest `BRIDGE_CHALLENGES_TOTAL`, `BRIDGE_SLASHES_TOTAL`, and `GOSSIP_LATENCY_BUCKETS`, `overlay_backend_active`, `overlay_peer_total`, and storage panels differentiate coder/compressor rollout via telemetry labels. Grafana and the snapshot generator now include a dedicated bridge row plotting five-minute `increase()` deltas for reward claims, approvals consumed, settlement results, and dispute outcomes so operators can audit bridge health without third-party widgets. | Bridge/VM anomaly detection still pending; dependency wrapper metrics not fully surfaced and overlay soak dashboards pending. |
| **Identity & Explorer** | 83.4 % | DID registry anchors with replay protection and optional provenance attestations, wallet and light-client commands support anchoring/resolving with sign-only/remote signer flows, explorer `/dids` endpoints expose history/anchor-rate charts with cached pagination, and governance archives revocation history alongside anchor data for audit. | Governance-driven revocation playbooks and mobile identity UX remain to ship. |
| **Economic Simulation & Formal Verification** | 43.0 % | Bench harness simulates inflation/demand; chaos tests capture seeds and the coder/compressor comparison harness exports throughput deltas for scenario planning. | Scenario coverage still thin and no integrated proof pipeline. |
| **Mobile UX & Contribution Metrics** | 73.2 % | Background sync respects battery/network constraints via platform-specific probes, persisted overrides, CLI/RPC gating messages, freshness-labelled telemetry embedded in log uploads, and operator toggles stored in `~/.the_block/light_client.toml`, while the encrypted mobile cache with TTL sweeping, restart replay, and flush tooling keeps offline transactions durable. | Push notifications, remote signer support, and broad hardware testing pending. |

## Immediate

- **Run fleet-scale QUIC chaos drills** – invoke `scripts/chaos.sh --quic-loss 0.15 --quic-dup 0.03` across multi-region clusters, harvest retransmit deltas via `sim/quic_chaos_summary.rs`, and extend `docs/networking.md` with mitigation guidance pulled from the new telemetry traces.
- **Document multisig signer payloads and release verification** – extend `docs/dex.md` and `docs/bridges.md` with the expanded signer-set schema, add release-verifier walkthroughs, update explorer guides, and ensure CLI examples mirror the JSON payload emitted by the wallet.
- **Publish treasury dashboard alerts** – explorer widgets remain pending; aggregator ingestion, warning surfaces, and documentation in `docs/governance.md` have landed alongside the new `gov.treasury.*` RPC coverage.
- **Automate release rollout alerting** – add explorer jobs that reconcile `release_history` installs against the signer threshold, publish Grafana panels for stale nodes, and raise alerts when `release_quorum_fail_total` moves without a corresponding signer update.
- **Stand up anomaly heuristics in the aggregator** – feed correlation caches into preliminary anomaly scoring, auto-request log dumps on clustered `quic_handshake_fail_total{peer}` spikes, and document the response workflow in `docs/monitoring.md`.
- **Ship operator rollback drills** – expand `docs/governance_release.md` with staged rollback exercises that rehearse `update::rollback_failed_startup`, including guidance for restoring prior binaries and verifying provenance signatures after a revert.
- **Operationalize DID anchors** – wire revocation alerts into explorer dashboards, expand `docs/identity.md` with recovery guidance, and ensure wallet/light-client flows surface governance revocations before submitting new anchors.

## Near Term

- **Operationalize SLA telemetry alerts** – wire `COMPUTE_SLA_PENDING_TOTAL`, `COMPUTE_SLA_NEXT_DEADLINE_TS`, and resolution feeds into the foundation dashboard alerts, surface automated outcomes in explorer timelines, and publish remediation guides for providers.
- **Range-boost mesh trials and mobile energy heuristics** – prototype BLE/Wi-Fi Direct relays, tune lighthouse multipliers via field energy usage, log mobile battery/CPU metrics, and publish developer heuristics.
- **Economic simulator runs for emission/fee policy** – parameterize inflation/demand scenarios, run Monte Carlo batches via bench-harness, report top results to governance, and version-control scenarios.
- **Compute-backed money and instant-app groundwork** – define redeem curves for CBM, prototype local instant-app execution hooks, record resource metrics for redemption, test edge cases, and expose CLI plumbing.

## Medium Term

- **Full cross-chain exchange routing** – implement adapters for SushiSwap and Balancer, integrate bridge fee estimators and route selectors, simulate multi-hop slippage, watchdog stuck swaps, and document guarantees.
- **Distributed benchmark network at scale** – deploy harness across 100+ nodes/regions, automate workload permutations, gather latency/throughput heatmaps, generate regression dashboards, and publish tuning guides.
- **Wallet ecosystem expansion** – add multisig modules, ship Swift/Kotlin SDKs, enable hardware wallet firmware updates, provide backup/restore tooling, and host interoperability tests.
- **Governance feature extensions** – roll out staged upgrade pipelines, support proposal dependencies and queue management, add on-chain treasury accounting, offer community alerts, and finalize rollback simulation playbooks.
- **Mobile light client productionization** – optimize header sync/storage, add push notification hooks for subsidy events, integrate background energy-saving tasks, support mobile signing, and run a cross-hardware beta program.

## Long Term

- **Smart-contract VM and SDK release** – design a deterministic instruction set with gas accounting, ship developer tooling and ABI specs, host example apps, audit and formally verify the stack.
- **Permissionless compute marketplace** – integrate heterogeneous GPU/CPU scheduling, enable provider reputation scoring, support escrowed cross-chain payments, build an SLA arbitration framework, and release marketplace analytics.
- **Global jurisdiction compliance framework** – publish regional policy packs, add PQ encryption, maintain transparency logs, allow per-region feature toggles, and run forkability trials.
- **Decentralized storage and bandwidth markets** – incentivize DHT storage, reward long-range mesh relays, integrate content addressing, benchmark large file transfers, and provide retrieval SDKs.
- **Mainnet launch and sustainability** – lock protocol parameters via governance, run multi-phase audits and bug bounties, schedule staged token releases, set up long-term funding mechanisms, and establish community maintenance committees.

## Next Tasks

1. **Implement governance treasury accounting**
   - Extend `node/src/governance/store.rs` with a `treasury_balances` table and checkpointed accruals.
   - Surface balances and disbursements via `rpc/governance.rs` plus CLI reporting.
   - Add regression coverage in `governance/tests/treasury_flow.rs` to confirm replay safety.
2. **Add proposal dependency resolution**
   - Encode prerequisite DAG edges in `node/src/governance/mod.rs` and persist them to the store.
   - Block activation in `controller::submit_release` until dependencies clear, logging failures through `release_quorum_fail_total`.
   - Document the workflow in `docs/governance.md` with explorer examples.
3. **Scale the QUIC chaos harness**
   - Allow `node/tests/quic_chaos.rs` to spawn multi-node meshes with seeded RNGs.
   - Export aggregated retransmit stats to `sim/quic_chaos_summary.rs` and archive representative traces for future tuning.
   - Update `scripts/chaos.sh` to accept topology manifests for repeatable WAN drills.
4. **Automate release rollout alerting**
   - Add an explorer cron that snapshots `release_history` and highlights nodes lagging more than one epoch.
   - Publish Grafana panels powered by `release_installs_total` and signer metadata.
   - Emit webhook alerts when installs stall beyond configurable thresholds.
5. **Stand up anomaly heuristics in the aggregator**
   - Feed correlation caches into a pluggable anomaly scoring engine within `metrics-aggregator`.
   - Persist annotations for later audit and surface them over the REST API.
   - Backstop behaviour with tests in `metrics-aggregator/tests/correlation.rs`.
6. **Enforce compute-market SLAs**
   - Introduce deadline tracking in `node/src/compute_market/scheduler.rs` and penalize tardy providers.
   - Record `compute_sla_violation_total` metrics and integrate with the reputation store.
   - Document remediation expectations in `docs/compute_market.md`.
7. **Prototype incentive-backed DHT storage**
   - Extend `storage_market` to price replicas, tracking deposits and proofs in `storage_market/src/lib.rs`.
   - Add explorer visibility into outstanding storage contracts and payouts.
   - Simulate churn within the `sim` crate to calibrate incentives before deployment.
8. **Deliver multisig wallet UX**
   - Layer multisig account abstractions into `crates/wallet` with CLI flows for key rotation and spending policies.
   - Ensure remote signer compatibility and persistence across upgrades.
   - Update `docs/wallets.md` with operator and end-user runbooks.
9. **Extend cross-chain settlement proofs**
   - Implement proof verification for additional partner chains in `bridges/src/light_client.rs`.
   - Capture incentives and slashable behaviour for relayers in `bridges/src/relayer.rs`.
   - Document settlement guarantees and failure modes in `docs/bridges.md`.
10. **Kick off formal verification pipeline**
    - Translate consensus rules into F* modules under `formal/consensus` with stub proofs.
    - Integrate proof builds into CI with caching to keep feedback fast.
    - Publish contributor guidelines in `formal/README.md` and schedule brown-bag sessions for new authors.
