# Dependency Inventory

_Last refreshed: 2025-11-02._  The workspace `Cargo.lock` no longer references
any crates from crates.io; every dependency in the graph is now first-party.
The final external cluster—the optional `legacy-format` sled importer—has been
replaced with an in-house manifest shim so the lockfile resolves solely to
workspace crates. `node/tests/read_ack_privacy` now leans on the existing
`concurrency::Lazy` helper for fixture reuse, keeping tests free of third-party
cells while trimming runtime.

| Tier | Crate | Version | Origin | License | Notes |
| --- | --- | --- | --- | --- | --- |
| _none_ | — | — | — | — | The workspace has zero third-party crates. |

## Highlights

- ✅ WAN chaos readiness stays in-house. `sim/src/chaos.rs` and the `chaos_lab` driver emit signed overlay/storage/compute attestations, and the `monitoring` crate (`monitoring/src/chaos.rs`) defines the codecs/verification helpers without pulling serde or external crypto. The metrics aggregator depends only on `monitoring-build` to parse `/chaos/attest`, stores snapshots in the in-house metrics store, and exposes `/chaos/status` plus the `chaos_readiness{module,scenario}`/`chaos_site_readiness{module,scenario,site}`/`chaos_sla_breach_total` gauges through the foundation metrics facade while logging `chaos_status_tracker_poisoned_recovering` on poisoned-lock recovery. CI wires the suite into `just chaos-suite` and `cargo xtask chaos`, both of which execute the first-party binaries—no third-party chaos tooling or HTTP clients required. A new regression (`chaos_lab_attestations_flow_through_status`) imports `tb-sim` as a dev-dependency to push the signed artefacts through `/chaos/attest` and assert `/chaos/status` plus metric updates end-to-end, and Grafana’s generated dashboards (`monitoring/src/dashboard.rs` → `monitoring/grafana/*.json`) now surface a dedicated **Chaos** row without involving external templating engines.
  - Follow-up coverage (`chaos_attestation_rejects_invalid_signature`) mutates payloads with the first-party crypto facade to ensure `/chaos/attest` rejects tampering, the gossip relay + peer metrics persistence layers fall back to in-memory stores when temp dirs or clocks fail, and mobile sync tests drop to a first-party stub whenever the optional runtime wrapper is disabled—no external filesystem helpers, HTTP clients, or runtime crates were required.
- ✅ Read-acknowledgement privacy proofs live entirely inside the workspace. The new `zkp` crate exposes readiness and acknowledgement commitments without pulling third-party SNARK libraries, and the node/gateway plumbing reuses existing hashing/serialization helpers while exposing `--ack-privacy` and `node.set_ack_privacy`.
- ✅ Deterministic liquidity routing lives in `node/src/liquidity/router.rs` and
  depends only on existing bridge/DEX/trust-line modules. Governance-configured
  batch size, fairness jitter, hop limits, and rebalance thresholds all flow
  through first-party config structs, and execution hands off to the in-tree
  bridge/Dex helpers—no external schedulers or crypto libraries introduced.
- ✅ Slack-aware trust routing reuses the same modules: the new
  `TrustLedger::max_slack_path` helper and hop-limited fallback logic rely
  exclusively on in-tree collections/iterators, so widening corridors and
  fallback selection required no third-party graph or optimisation crates.
- ✅ Bridge CLI RPC calls now flow through a new `BridgeRpcTransport` trait that
- ✅ Bridge CLI parser regressions now cover settlement-log asset filters, reward-accrual relayer/asset cursors, and default pagination via the first-party `Parser`, while `bridge_pending_dispute_persists_across_restart` keeps dispute persistence tests inside the sled-backed bridge crate. Monitoring’s `dashboards_include_bridge_remediation_legends_and_tooltips` guards Grafana legends/descriptions without third-party validators.
  wraps the production `RpcClient` while letting tests inject an in-memory
  `MockTransport`. The HTTP-based `JsonRpcMock` harness and async runtime
  dependency disappeared from `cli/tests`, keeping FIRST_PARTY_ONLY runs
  hermetic without background servers.
- ✅ Bridge dispute audit regressions now drive the command builder through the
  first-party `Parser` and transport, asserting optional `asset`/`cursor`
  filters serialise to JSON `null`, the default 50-row page size remains intact,
  and the localhost RPC fallback survives future refactors. Monitoring’s
  `dashboards_include_bridge_counter_panels` helper parses every generated
  Grafana JSON (dashboard/operator/telemetry/dev) to ensure the reward-claim,
  approval, settlement, and dispute panels preserve their first-party queries
  and legends across templates—no third-party validators or dashboard tooling
  introduced.
- ✅ Bridge remediation regressions now allocate a per-test `RemediationSpoolSandbox` using `sys::tempfile`, seeding isolated spool directories for page/throttle/quarantine/escalate targets and exercising `remediation_spool_sandbox_restores_environment` so scoped `TB_REMEDIATION_*_DIRS` guards tear down automatically. Retry-heavy suites stay hermetic with zero `/tmp` residue and no third-party harnesses.
- ✅ Runtime integration suites explicitly allow `clippy::unwrap_used`/`expect_used`
  in test modules and guard histogram bucket sorting against NaNs, eliminating the
  lint debt that previously blocked workspace `cargo clippy` runs—no external
  lint suppressors or forks required.
- ✅ Explorer `/blocks/:hash/payouts` and the matching CLI command reuse the
  first-party SQLite/JSON codecs to emit per-role read/ad totals. Tests insert
  JSON directly, avoiding serde stubs while staying in-tree, and the new
  `ad_read_distribution` node integration mines blocks through the native
  ledger—no external analytics or database dependencies introduced.
- ✅ Legacy payout snapshots now ride the same in-house codecs: explorer unit
  tests cover the JSON fallback without serde_json, CLI error paths stay inside
  the first-party transport stack, and the Grafana generator renders the new
  “Block Payouts” row via the existing dashboard builder—no third-party charting
  or parsing libraries added.
- ✅ Explorer payout caches now clamp regressions inside `metrics_aggregator::record_explorer_payout_metric`, logging trace-only diagnostics, and the churn plus peer-isolation regressions (`explorer_payout_counters_remain_monotonic_with_role_churn`, `explorer_payout_counters_are_peer_scoped`) drive alternating read/advertising role sets and disjoint peers entirely through the first-party HTTPd test harness—no external metrics helpers required.
- ✅ Bridge remediation spool artefacts now persist across acknowledgement
  retries and are drained automatically once hooks acknowledge or close an
  action. Restart suites assert the cleanup path, the contract remediation CLI
  exposes per-action `spool_artifacts` in filtered/JSON output, and monitoring
  now guards both the latency policy overlays and the new
  `bridge_remediation_spool_artifacts` gauge/panel—all without introducing
  third-party tooling.
- ✅ The remediation engine now automates retries and escalations without leaving
  the first-party stack. Actions persist dispatch attempts, retry counts, and
  acknowledgement metadata, the parser tolerates plain-text hook responses, and
  alerting consumes the stored acknowledgement counter to flag pending or missing
  closures—all without introducing third-party schedulers or JSON tooling.
- ✅ CLI wallet integration tests now snapshot the `signer_metadata` array across
  ready, override, ephemeral, and session preview flows. The `fee_floor_warning`
  suite asserts the struct-level metadata vector, and the new
  `wallet_signer_metadata` module inspects the auto-bump telemetry event—all via
  first-party `JsonMap` builders—so the CLI remains hermetic without mock RPC
  clients while guaranteeing deterministic JSON output for FIRST_PARTY_ONLY
  runs.
- ✅ Bridge incentives, duty tracking, and relayer accounting now live entirely on the first-party stack. `BridgeIncentiveParameters` moved into the shared `bridge-types` crate, sled-backed snapshots in `node/src/bridge/mod.rs` encode via handwritten JSON helpers, and the new RPC/CLI surfaces (`bridge.relayer_accounting`, `bridge.duty_log`, `blockctl bridge accounting`, `blockctl bridge duties`) expose rewards/penalties without touching serde fallbacks. Integration suites `node/tests/bridge.rs` and `node/tests/bridge_incentives.rs` cover honest/faulty relayers and governance overrides under `FIRST_PARTY_ONLY`.
- ✅ Governance-backed bridge reward claims, accruals, and settlement proofs run entirely on
  first-party helpers. `bridge.claim_rewards`, `bridge.reward_claims`,
  `bridge.reward_accruals`, `bridge.submit_settlement`, `bridge.settlement_log`,
  and `bridge.dispute_audit` share typed request/response structs and manual JSON
  builders mirrored by the CLI (`blockctl bridge claim`, `reward-claims`,
  `reward-accruals`, `settlement`, `settlement-log`, `dispute-audit`).
  Cursor/limit pagination with `next_cursor` responses keeps long histories
  streaming through the same builders, deterministic settlement digests/height
  watermarks block hash/height replays, and channel configuration accepts
  optional fields/clear flags without serde fallbacks. New unit tests in
  `governance/src/store.rs` plus `node/src/governance/store.rs` validate sled
  persistence end to end while `node/tests/bridge_incentives.rs` covers the new
  accrual ledger.
- ✅ Treasury CLI tests now exercise the helper builders directly: lifecycle
  coverage funds the sled-backed store, execution/cancel assertions inspect the
  typed records, and remote fetch regressions validate
  `combine_treasury_fetch_results` with and without history—no
  `JsonRpcMock` servers or `foundation_serialization::json::to_value`
  conversions remain in the suite.
- ✅ Wallet build previews expose signer metadata through a new
  `BuildTxReport::signer_metadata` field, and the accompanying unit tests assert
  on the JSON projection for auto-bump, confirmation, ephemeral, and session
  flows while snapshotting the serialized array. The
  service-badge and telemetry CLI modules now ship helper-backed tests that
  snapshot the JSON-RPC envelopes for `verify`/`issue`/`revoke` and
  `telemetry.configure`, eliminating reliance on mock servers or serde
  conversions. The mobile push notification and node difficulty examples were
  manualized as well, replacing their last `foundation_serialization::json!`
  literals with explicit `JsonMap` builders so documentation tooling stays on
  the first-party facade.
- ✅ Contract CLI JSON surfaces now share a first-party `json_helpers` module.
  Compute, service-badge, scheduler, telemetry, identity, config, bridge, and
  TLS commands all emit RPC payloads through explicit `JsonMap` builders, and
  the node runtime log sink plus the staking/escrow wallet binary reuse the same
  helpers. This removes every remaining `foundation_serialization::json!`
  literal from the operator tooling stack while preserving legacy response
  shapes and deterministic field ordering.
- ✅ Governance webhooks now post via the first-party HTTP client regardless of
  the telemetry feature flag, and the CLI/node networking utilities (`contract
  net`, gateway mobile-cache, light-client device status, wallet send, and
  `node/src/bin/net.rs`) replaced every `foundation_serialization::json!`
  literal with explicit `JsonMap` builders and a shared `RpcRequest` envelope,
  eliminating serde-backed macro usage from the hot RPC tooling paths.
- ✅ Mempool admission now derives a priority tip from `payload.fee` when the
  caller omits one, keeping legacy tooling compatible with the lane floor while
  staying entirely inside the first-party cursor helpers. Governance retuning
  replaced its serde-derived `KalmanState` serializer with manual
  `json::Value` parsing/encoding so industrial multiplier history persists via
  the in-house JSON facade.
- ✅ RPC fuzzing now routes through the first-party `foundation_fuzz`
  harness and `fuzz_dispatch_request`, removing the last reliance on
  test-only RPC internals.
- ✅ Ledger persistence and startup rebuild now consume the cursor-backed
  `ledger_binary` helpers end to end: `MempoolEntryDisk` stores a cached
  `serialized_size`, the rebuild path uses it before re-encoding, and new unit
  tests cover `decode_block_vec`, `decode_account_map_bytes`, and
  `decode_emission_tuple` so no `binary_codec` fallbacks remain for legacy
  snapshots.
- ✅ The node RPC client now emits JSON-RPC envelopes through manual
  `foundation_serialization::json::Value` builders and decodes responses without
  invoking `foundation_serde` derives, preventing the stub backend from firing
  during `mempool`/`stake`/`inflation` client calls.
- ✅ Storage provider-profile compatibility tests now rely on the cursor writer
  that production code uses, dropping the last `binary_codec::serialize`
  invocation from the suite while preserving randomized EWMA/throughput checks.
- ✅ Gossip peer telemetry tests and the aggregator failover harness assert
  against the shared `peer_snapshot_to_value` helper, keeping networking JSON
  construction entirely first party during CI runs.
- ✅ Node runtime logging and governance webhooks now build payloads via explicit
  first-party helpers (`node/src/bin/node.rs`, `node/src/telemetry.rs`), removing
  the last `foundation_serialization::json!` invocations from production
  binaries and keeping log sinks/webhook alerts on the deterministic JSON facade.
- ✅ Peer statistics RPC responders now construct their JSON payloads through
  deterministic first-party builders instead of `foundation_serialization::json::to_value`,
  so `net.peer_stats_export_all` exports stay on the in-house stack and avoid
  serde-backed conversions.
- ✅ Compute-market scheduler/job capability responders and DEX escrow RPCs now
  assemble payloads with first-party `Value` builders. Payment proofs, Merkle
  roots, utilization maps, and capability snapshots no longer touch
  `json::to_value`, keeping those surfaces on the in-house JSON facade while
  preserving the legacy response layout.
- ✅ `foundation_fuzz::Unstructured` grew native IP address helpers plus unit
  coverage, simplifying network-oriented fuzz targets.
- ✅ The optional sled legacy importer is now implemented in-house; enabling the
  feature consumes a JSON manifest instead of pulling the crates.io `sled`
  stack, so `FIRST_PARTY_ONLY=1` builds cover the entire workspace.
- ✅ Gossip messages, ledger blocks, and transactions now encode via
  `net::message`, `transaction::binary`, and `block_binary` cursor helpers,
  removing the remaining `binary_codec` shim usage while new tests lock payload
  order and legacy parity across handshake/drop maps and DEX/storage manifests.
- ✅ Those cursor writers now delegate to `StructWriter::write_struct` with
  `field_u8`/`field_u32` shorthands so layout metadata stays inline, eliminating
  the manual field counts that previously produced `Cursor(UnexpectedEof)` when
  schemas drifted and reducing boilerplate for future codecs.
- ✅ Canonical transaction helpers (`canonical_payload_bytes`,
  `verify_signed_tx`, CLI signing, and the Python wrappers) now reuse the
  cursor encoders directly. `codec::serialize` is no longer invoked for
  `RawTxPayload`/`SignedTransaction`, removing the last runtime paths that hit
  the `foundation_serde` stub during admission or fee regression tests.
- ✅ Net and gateway fuzz harnesses dropped `libfuzzer-sys`/`arbitrary`
  in favour of the shared modules and now ship smoke tests that exercise
  the in-tree entry points directly.
- ✅ `foundation_serde` and `foundation_qrcode` no longer expose external
  backends; every consumer—including the remote signer CLI—now relies on
  the stubbed first-party implementations.
- 🚧 Keep regenerating this inventory after large dependency refactors so the
  dashboard and summaries remain accurate.
