# Summary
> **Review (2025-11-08, evening):** Ad settlements now honour the
> `liquidity_split_ct_ppm` governance knob. The marketplace converts the CT slice
> of liquidity before minting tokens, preventing the IT allocation from being
> double counted, and refreshed tests assert the split alongside the USD→CT/IT
> conversions. Explorer and ledger pipelines consume the corrected
> `SettlementBreakdown` directly, so dashboards and CI artefacts display matching
> USD, CT, and IT totals. Readiness telemetry mirrors the same inputs: snapshots
> persist both the archived and live marketplace oracles, expose per-cohort
> utilisation deltas, and feed the Prometheus exporters plus metrics aggregator
> (`ad_readiness_utilization_{observed,target,delta}_ppm`) so CI and dashboards can
> page when utilisation drifts from the governance targets despite steady demand.
> **Review (2025-10-27, evening):** Chaos artefacts now ship with preserved
> manifests and first-party publishing hooks. `sim/chaos_lab.rs` writes a
> run-scoped `manifest.json` alongside a `latest.json` pointer under
> `chaos/archive/`, records BLAKE3 digests and byte sizes for every snapshot,
> diff, overlay, and provider-failover file, and bundles the set into a
> `run_id.zip` archive. Optional `--publish-dir`, `--publish-bucket`, and
> `--publish-prefix` flags mirror the manifests and bundle into downstream
> directories or S3-compatible object stores through the
> first-party `foundation_object_store` client, which now ships a
> canonical-request regression and blocking upload harness proving AWS Signature
> V4 headers match the published examples while honouring
> `TB_CHAOS_ARCHIVE_RETRIES` (minimum 1) and optional
> `TB_CHAOS_ARCHIVE_FIXED_TIME` timestamps. `tools/xtask` consumes the manifests via
> manual `foundation_serialization::json::Value` decoding, surfaces publish
> targets, logs the manifest/bundle BLAKE3 digests and byte sizes, and honours the
> new flags so release automation and dashboards see identical artefact
> inventories. `scripts/release_provenance.sh` refuses to continue unless
> `chaos/archive/latest.json` and the referenced run manifest exist, while
> `scripts/verify_release.sh` parses the manifest to ensure every archived file is
> present, that bundle sizes match on-disk artefacts, and that mirrored paths
> referenced by `cargo xtask chaos` resolve, closing the loop without third-party
> tooling.
> **Review (2025-10-27, afternoon):** `/chaos/status` baselines now flow entirely
> through first-party tooling. `sim/chaos_lab.rs` pulls snapshots with
> `httpd::BlockingClient`, decodes them manually via
> `foundation_serialization::json::Value`, and persists overlay readiness rows so
> soak automation can diff provider-aware regressions without serde stubs or
> third-party HTTP clients. `cargo xtask chaos` consumes the emitted JSON through
> the same facade, reporting module totals, scenario readiness, provider churn,
> readiness improvements/regressions, and duplicate site detection using only
> `std` collections. The existing provider-labelled gauges, signed attestations,
> and bind-warning unification remain in place, giving operators a fully
> first-party chaos loop from harness to dashboards to CI gating.
> **Review (2025-10-26, late night):** Mixed-provider chaos rehearsals now feed
> per-site readiness into the aggregator. The simulator’s overlay scenarios wire
> provider weights and latency penalties into `ChaosSite` entries so
> `chaos_lab` exports site-level readiness vectors, and `/chaos/status` returns
> those arrays alongside module rollups while the new
> `chaos_site_readiness{module,scenario,site,provider}` gauge tracks them for dashboards
> and automation. The aggregator hardens `/chaos/attest` against poisoned locks,
> warns when the status tracker must recover from a mutex poison, and keeps the
> site gauges sorted to produce stable Grafana/JSON snapshots. Mobile sync tests
> now fall back to a first-party stub whenever the runtime wrapper feature is
> disabled, preserving lint/test runs without importing a third-party client,
> and a `Node::start` integration test binds an occupied port to prove the
> gossip listener surfaces a warning instead of panicking when sockets are
> unavailable.
> **Review (2025-12-14, afternoon):** The autonomous WAN chaos lab now ships as a
> first-party binary (`sim/chaos_lab.rs`) backed by deterministic overlay/storage
> /compute scenarios. Signed readiness attestations feed directly into the
> metrics aggregator via `/chaos/attest`, which verifies the payloads, surfaces
> `/chaos/status`, and exports `chaos_readiness{module,scenario}` alongside
> `chaos_sla_breach_total` for dashboards and CI gating. `just chaos-suite` and
> `cargo xtask chaos` now gate releases, and the aggregator’s remediation tests
> hold a global dispatch-log guard so concurrent suites stay hermetic. Grafana’s
> auto-generated dashboard gained a dedicated **Chaos** row charting readiness and
> five-minute breach deltas, while a new integration test pipes the `chaos_lab`
> artefacts through `/chaos/attest` end-to-end to assert `/chaos/status` and metric
> updates with the signer digest preserved. The chaos pipeline now rejects tampered
> signatures in dedicated regression coverage, and both the gossip shard cache and
> peer metrics store fall back to in-memory paths (or skip persistence) instead of
> panicking when system clocks or scratch directories misbehave. The `sim/did.rs`
> driver now renders DID documents through first-party JSON builders, sidestepping
> the serde stub and keeping the binary panic-free under full crate testing.
> **Review (2025-10-26, night):** Distributed chaos site overrides now flow from
> `TB_CHAOS_SITE_TOPOLOGY` through the simulator and monitoring dashboards,
> exposing `chaos_site_readiness{module,site}` alongside module readiness and SLA
> deltas. The aggregator rejects malformed module labels and truncated byte
> arrays before mutating readiness gauges, gossip nodes return
> `io::Result<JoinHandle>` on startup, and `load_net_key` logs persistence
> failures instead of panicking so WAN chaos rehearsals continue even when bind or
> fsync operations fail.
> **Review (2025-10-26, late morning):** Liquidity router coverage now exercises
> multi-batch fairness, slack-aware trust routing, and hop-constrained fallbacks.
> Integration tests prove challenged withdrawals never execute, excess DEX
> intents roll deterministically into follow-up batches, and the router downgrades
> to the shortest-path fallback when governance hop limits would reject the
> slack-optimised route. The DEX documentation captures the new search heuristic
> and test plan so operators understand why wider corridors may pre-emptively
> win when they safeguard future capacity.
> **Review (2025-11-07, morning):** Peer telemetry registration now routes
> through shared helpers that log metric/label combinations when registration
> fails instead of aborting the process. Networking keeps processing peer events
> even when labels drift, and operators see structured warnings tagged with the
> offending metric to chase down configuration issues without downtime.
> **Review (2025-11-02, morning):** A deterministic, first-party liquidity router
> now sequences DEX escrows, bridge withdrawals, and trust-line rebalances
> through `node/src/liquidity/router.rs`. Governance configures batch size,
> fairness jitter, hop limits, and rebalance thresholds while bridge finalisers
> honour the router’s ordering so cross-chain FX stays MEV-resistant. Runtime’s
> integration suites gained explicit `clippy::unwrap_used`/`expect_used`
> allowances and NaN guards, clearing the longstanding full-workspace lint debt.
> **Review (2025-10-26, evening):** Read-ack integration tests reuse a shared
> fixture through the first-party `concurrency::Lazy` helper, trimming duplicate
> RNG/proof setup while keeping `invalid_privacy` detection coverage intact.
> Concurrent reservation tests now prove the in-memory and sled marketplaces
> hold their pending-budget locks through insertion so campaigns are never
> oversubscribed, and the compute dashboard adds a "Read Ack Outcomes" panel to
> surface the new `read_ack_processed_total{result="invalid_privacy"}` series.
> **Review (2025-10-25, late evening):** Read acknowledgements now ship readiness
> and identity proofs via the first-party `zkp` crate. Operators can toggle
> enforcement with `--ack-privacy` or the `node.{get,set}_ack_privacy` RPCs, and
> telemetry surfaces `read_ack_processed_total{result="invalid_privacy"}` when
> observe mode spots mismatched proofs. Advertising reservations include a
> per-ack discriminator so identical fetches no longer overwrite each other.
> **Review (2025-11-01, early morning):** Bridge audit tooling stays hermetic—new
> CLI regressions run the dispute-audit builder through the first-party parser,
> confirm optional filters serialise to JSON `null`, and monitoring parses every
> Grafana variant to ensure the bridge reward/approval/settlement/dispute panels
> retain their first-party queries.
> **Review (2025-10-31, late evening):** Bridge remediation regressions promote the `RemediationSpoolSandbox` helper to cover every `TB_REMEDIATION_*_DIRS` target (page, throttle, quarantine, escalate) and ship an explicit environment-restoration test so retry suites can assert the guards cleanly unwind. Explorer payout coverage now stacks a peer-isolation scenario—`explorer_payout_counters_are_peer_scoped` proves per-peer caches remain monotonic while alternating read/advertising labels—and the churn regression still locks in the mixed-role baseline behaviour without negative deltas.
> **Review (2025-10-31, afternoon):** Bridge remediation integration suites now allocate a per-test `RemediationSpoolSandbox` so spool hooks write into isolated, auto-cleaned directories with environment guards restoring `TB_REMEDIATION_*_DIRS`/`*_URLS` after each run. Explorer payout ingestion learned to treat alternating read/advertising role sets as monotonic—the aggregator clamps regressions to the previous high with trace-only diagnostics—and the new `explorer_payout_counters_remain_monotonic_with_role_churn` regression keeps cache baselines hermetic when peers churn across scrapes.
> **Review (2025-11-06, afternoon):** Dual-token advertising payouts now surface everywhere the ledger reports settlements. Genesis, block, and ledger codecs persist CT totals, IT totals, USD micros, settlement counts, and oracle snapshots, while explorer/CLI payloads render the expanded fields with integration tests across binary and JSON codecs. The metrics aggregator introduced `explorer_block_payout_ad_it_total{role}` plus peer-labelled gauges for advertising USD totals, settlement counts, and CT/IT oracle prices, and `ad_market.readiness` exposes the archived snapshot, live oracle values, and a cohort `utilization` summary under first-party codecs. Dashboards, CI artefacts, and Prometheus scrape regressions consume the same gauges so operators can audit conversion inputs without digging into raw JSON.
> **Review (2025-10-30, morning):** Explorer payout lookups now cover legacy snapshots that omit the per-role headers, with unit tests exercising the JSON fallback so FIRST_PARTY_ONLY runs never lose visibility. The CLI’s `explorer block-payouts` command surfaces clear errors when hashes or heights are missing/mismatched, and the monitoring stack picked up a dedicated “Block Payouts” row charting the read-subsidy and advertising role splits from Prometheus. These additions keep the payout trail hermetic—from historic blocks through the CLI to Grafana—without leaning on third-party codecs or dashboards.
> **Review (2025-10-27, late afternoon):** Bridge remediation spool artefacts now
> persist across acknowledgement retries, drain automatically once hooks close
> or acknowledge the action, and restart tests verify the cleanup path. The
> contract CLI’s JSON output exposes per-action `spool_artifacts` for filtered
> playbook/peer combinations, and the monitoring suite now guards both the
> latency policy overlays and the new `bridge_remediation_spool_artifacts`
> gauge/panel charting outstanding spool payloads.
> **Review (2025-10-27, morning):** The bridge dashboards now overlay the policy
> gauge `bridge_remediation_ack_target_seconds{playbook,policy}` on the latency
> histogram, the metrics aggregator restores acknowledgement samples after
> restarts, and a new `BridgeRemediationAckLatencyHigh` alert flags p95 latency
> above the configured target before escalations fire. The `contract remediation
> bridge` CLI added `--playbook`, `--peer`, and `--json` flags so operators and
> automation can filter or ingest persisted actions without leaving the
> first-party tooling.
> **Review (2025-10-25, late evening):** Per-playbook acknowledgement windows
> honour `TB_REMEDIATION_ACK_*` overrides, acknowledgement completion latency is
> recorded in `bridge_remediation_ack_latency_seconds{playbook,state}` (with
> p50/p95 panels in Grafana/HTML snapshots), and the first-party `contract
> remediation bridge` command streams the persisted action/dispatch log for
> on-call triage.

Bridge remediation highlights:
- `BridgeRemediationAckPolicy` reads `TB_REMEDIATION_ACK_RETRY_SECS`,
  `_ESCALATE_SECS`, `_MAX_RETRIES`, and suffix overrides (for example,
  `_GOVERNANCE_ESCALATION`) so paging, throttling, and escalation playbooks use
  tailored retry/escalation windows.
- `bridge_remediation_ack_latency_seconds{playbook,state}` captures acknowledgement
  completion latency; Grafana and the HTML snapshot chart p50/p95 values alongside
  the policy gauge `bridge_remediation_ack_target_seconds{playbook,policy}` and
  persist samples across restarts so slow closures surface ahead of policy
  triggers.
- `contract remediation bridge --aggregator <url>` renders the persisted
  actions, acknowledgement metadata, retry history, follow-up notes, and the
  dispatch log without leaving the first-party CLI. Filter output with
  `--playbook`/`--peer` or stream JSON via `--json` when automation needs the
  same view.
> **Review (2025-10-25, afternoon):** Remediation follow-ups are now automated
> end-to-end. Pending actions track dispatch attempts, retry counts, and follow-up
> notes so the aggregator can re-dispatch playbooks before synthesising governance
> escalations when policy thresholds expire. The acknowledgement parser accepts
> plain-text hook responses alongside JSON, and new alerts
> (`BridgeRemediationAckPending`, `BridgeRemediationClosureMissing`) page when
> acknowledgements stall or closures never arrive, keeping the paging/escalation
> loop entirely first party.
> **Review (2025-10-25, mid-morning):** Governance escalations now report
> acknowledgement state end to end. The metrics aggregator records
> `bridge_remediation_dispatch_ack_total{action,playbook,target,state}`, stores
> acknowledgement/closure timestamps and notes on each remediation action, and a
> new Grafana panel charts acknowledgement deltas next to dispatch totals so
> operators can confirm downstream paging/governance hooks closed the loop. The
> bridge anomaly test suite drives acknowledgement paths through an in-process
> HTTP override, keeping FIRST_PARTY_ONLY coverage hermetic.
> **Review (2025-10-25, early morning):** Bridge remediation payloads now ship
> annotations, dashboard hints, and response sequences, while the aggregator
> exposes `/remediation/bridge/dispatches` so paging/governance hooks can audit
> per-target outcomes. Operator runbooks link directly to the generated steps,
> and the alert validator adds dispute/quorum recovery fixtures to keep the skew
> heuristics calm during recovery windows.
> **Review (2025-10-24, late evening):** Bridge dispatch hooks now emit
> `bridge_remediation_dispatch_total{action,playbook,target,status}` alongside the
> existing action counter, with CLI integration tests covering spool successes,
> failures, and skipped hooks. Grafana and the HTML snapshot gained a dispatch
> health panel, and the incident playbook points operators at the new legend
> entries for rapid diagnostics.
> **Review (2025-10-24, midday):** Bridge remediation actions fan out to first-party paging/governance hooks via `TB_REMEDIATION_*_URLS`/`*_DIRS`. The aggregator logs each dispatch, persists spool payloads, and the operator incident playbook ties the remediation panel to the dispatched playbook. Alert validation now includes recovery-curve and partial-window fixtures so the `BridgeCounter*Skew` heuristics remain stable across edits.
> **Review (2025-10-24, pre-dawn):** Bridge anomaly remediation now persists
> page/throttle/quarantine/escalation decisions, exposes `/remediation/bridge`
> plus the `bridge_remediation_action_total{action,playbook}` counter, and
> resumes state after restarts so operators can trigger first-party mitigations
> directly from the metrics aggregator. Alert validation moved into the shared
> `monitoring/src/alert_validator.rs` helper, and the existing
> `bridge-alert-validator` binary now replays canned datasets for bridge,
> chain-health, dependency-registry, and treasury groups so CI guards every
> Prometheus expression without promtool.
> **Review (2025-10-23, late evening):** Bridge alerting now includes
> label-specific coverage and a first-party validator. New Prometheus rules
> `BridgeCounterDeltaLabelSkew`/`BridgeCounterRateLabelSkew` watch
> `labels!=""` selectors for 3× deviations above the 30-minute baseline while
> preserving the aggregate alerts. The validator binary parses
> `monitoring/alert.rules.yml`, verifies the expressions, and replays canned data;
> CI runs it alongside `cargo test --manifest-path monitoring/Cargo.toml` so the
> bridge alert group stays hermetic without promtool.
> **Review (2025-10-22, late evening):** Bridge monitoring tightened across the
> stack. The metrics aggregator now persists `bridge_metric_delta`/`bridge_metric_rate_per_second`
> baselines to the in-house store and the new restart regression confirms the
> gauges resume from the previous watermark. Prometheus alert rules
> (`BridgeCounterDeltaSkew`, `BridgeCounterRateSkew`) watch those gauges for
> 3× deviations above the 30-minute average, and the Grafana templates plus HTML
> snapshot render dedicated panels so the alerts link directly to first-party
> timeseries.
> **Review (2025-10-23, afternoon):** Contract CLI bridge commands now depend on a
> trait-backed `BridgeRpcTransport`, letting production reuse the in-house
> `RpcClient` while tests inject an in-memory mock to capture JSON envelopes.
> The HTTP `JsonRpcMock` harness and runtime dependency are gone, yet the suite
> still covers reward-claim pagination, settlement submissions, and dispute
> audits via `handle_with_transport` under FIRST_PARTY_ONLY.
> **Review (2025-10-22, late evening):** Bridge incentives now persist through a shared `bridge-types` crate and sled-backed duty/accounting ledgers in `node/src/bridge/mod.rs`. Manual JSON encoders expose the state to RPC/CLI consumers (`bridge.relayer_accounting`, `bridge.duty_log`, `blockctl bridge accounting`, `blockctl bridge duties`), and integration coverage in `node/tests/bridge.rs` plus `node/tests/bridge_incentives.rs` exercises honest/faulty relayers, governance overrides, and challenge penalties under `FIRST_PARTY_ONLY`.
> **Review (2025-10-22, mid-morning+):** CLI wallet tests now snapshot signer
> metadata end-to-end: the `fee_floor_warning` integration suite asserts the
> JSON metadata vector for ready and override previews, and a dedicated
> `wallet_signer_metadata` module covers local, ephemeral, and session signers
> while verifying the auto-bump telemetry event. These tests operate on
> first-party `JsonMap` builders, keeping the suite hermetic without mock RPC
> servers and ensuring the new metadata stays stable for FIRST_PARTY_ONLY runs.
> **Review (2025-10-22, early morning):** Wallet build previews now emit and test
> signer metadata alongside payloads, giving JSON consumers a deterministic view
> of auto-bump, confirmation, ephemeral, and session sender states while
> snapshotting the JSON array for future regressions. Service-badge and
> telemetry commands gained helper-backed unit tests that assert on the
> JSON-RPC envelopes for `verify`/`issue`/`revoke` and `telemetry.configure`,
> keeping the CLI suite entirely first-party. The mobile notification and node
> difficulty examples have been manualized as well, dropping their last
> `foundation_serialization::json!` calls in favour of shared `JsonMap`
> builders so docs tooling mirrors production behaviour. `cargo test
> --manifest-path cli/Cargo.toml` passes with the new metadata assertions.
> **Review (2025-10-21, evening++):** Treasury CLI helpers now power every test
> scenario directly. Lifecycle coverage funds the sled-backed store before
> execution, asserts on typed status transitions, and avoids
> `foundation_serialization::json::to_value`, while remote fetch regression
> tests exercise `combine_treasury_fetch_results` with and without balance
> history to guarantee deterministic JSON assembly. The suite runs to completion
> without `JsonRpcMock` servers, and the node library tests were rerun to green
> to confirm the CLI refactor leaves runtime responders untouched.
> **Review (2025-10-21, mid-morning):** The contract CLI now exposes a shared
> `json_helpers` module that centralizes first-party JSON builders and RPC
> envelope helpers. Compute, service-badge, scheduler, telemetry, identity,
> config, bridge, and TLS commands build payloads through explicit `JsonMap`
> assembly, governance listings serialize through a typed wrapper, and the node
> runtime log sink plus the staking/escrow wallet binary reuse the same helpers.
> This removes the last `foundation_serialization::json!` macros from operator
> tooling while preserving legacy response shapes and deterministic ordering.
> **Review (2025-10-21, pre-dawn):** Governance webhooks now honour
> `GOV_WEBHOOK_URL` regardless of the `telemetry` feature flag, so operators
> receive activation/rollback notifications even on minimal builds. The CLI
> networking stack (`net`, `gateway`, `light_client`, `wallet`) introduces a
> reusable `RpcRequest` helper and explicit `JsonMap` assembly, replacing
> `foundation_serialization::json!` literals with deterministic first-party
> builders; the node’s `net` binary mirrors the change for peer stats, exports,
> and throttle calls. These manual builders keep FIRST_PARTY_ONLY pipelines clean
> while retaining the existing RPC shapes.
> **Review (2025-10-20, near midnight):** Admission now backfills a priority
> tip when callers omit one by subtracting the live base fee from
> `payload.fee`, keeping legacy builders compatible with the lane floor and
> restoring the base-fee regression under FIRST_PARTY_ONLY. Governance
> retuning no longer touches `foundation_serde`: Kalman state snapshots decode
> via `json::Value` and write through a first-party map builder, so the
> inflation pipeline stays entirely on the in-house JSON facade.
> **Review (2025-10-20, late evening):** Canonical transaction payloads now
> serialise exclusively through the cursor helpers. Node `canonical_payload_bytes`
> forwards to `encode_raw_payload`, signed-transaction hashing reuses the manual
> writer, the Python bindings decode via `decode_raw_payload`, and the CLI signs
> by converting into the node struct before calling the same encoder. The
> `foundation_serde` stub is no longer touched during RawTxPayload admission,
> unblocking the base-fee regression under FIRST_PARTY_ONLY.
> **Review (2025-10-20, afternoon++):** Peer metric JSON helpers sort drop and
> handshake maps deterministically, with new unit tests guarding the ordering
> as we phase out bespoke RPC builders. Compute-market RPC endpoints for
> scheduler stats, job requirements, provider hardware, and the settlement audit
> log now construct payloads entirely through first-party `Value` builders, so
> capability snapshots, utilization maps, and audit records no longer rely on
> `json::to_value`. DEX escrow status/release responses serialize payment proofs
> and Merkle roots manually, matching the legacy array layout while keeping the
> entire surface inside the in-house JSON facade.
> **Review (2025-10-20, midday):** Block, transaction, and gossip codecs now
> build their structs through `StructWriter::write_struct` with the new
> `field_u8`/`field_u32` helpers, eliminating hand-counted field totals that
> previously surfaced as `Cursor(UnexpectedEof)` during round-trip tests. RPC
> peer metrics dropped `json::to_value` conversions in favour of deterministic
> builders, so `net.peer_stats_export_all` stays fully on the first-party JSON
> stack. Fresh round-trip coverage exercises the updated writers for block,
> blob-transaction, and gossip payloads under the cursor facade.
> **Review (2025-10-20, morning):** Ledger persistence, mempool rebuild, and
> legacy decode paths now run purely on the `ledger_binary` cursor helpers.
> `MempoolEntryDisk` stores a cached `serialized_size`, startup rebuild consumes
> it before re-encoding, and new tests cover the block/account/emission decoders
> plus the five-field mempool layout so FIRST_PARTY_ONLY runs stay green without
> `binary_codec` fallbacks.
> **Review (2025-10-19, midday):** The node RPC client now builds JSON-RPC
> envelopes through manual `json::Value` maps and parses acknowledgements
> without relying on `foundation_serde` derives. `mempool.stats`,
> `mempool.qos_event`, `stake.role`, and `inflation.params` requests reuse the
> shared builder helpers, so FIRST_PARTY_ONLY builds no longer trigger stub
> panics when issuing or decoding client calls.
> **Review (2025-10-19, early morning):** Gossip and ledger surfaces now emit
> binary payloads exclusively through the in-house cursor helpers. The new
> `net::message` encoder signs/serializes every payload variant (handshake, peer
> rotation, transactions, blob chunks, block/chain broadcasts, reputation
> updates) with comprehensive tests, while `transaction::binary` and
> `block_binary` replace legacy serde/bincode shims for raw payloads, signed
> transactions, blob transactions, and blocks. Cursor-backed regression tests
> cover quantum and non-quantum builds, and DEX/storage manifest fixtures now
> inspect cursor output directly instead of depending on `binary_codec`, keeping
> sled snapshots firmly first party.
> **Review (2025-10-16, evening++)**: `foundation_serialization` now passes its
> full stub-backed test suite. The refreshed `json!` macro handles nested
> literals and identifier keys, binary/TOML fixtures ship handwritten
> serializers, and the `foundation_serde` stub exposes direct primitive visitors
> so tuple decoding no longer panics under FIRST_PARTY_ONLY. Serialization docs
> and guards now treat the facade as fully first party.
> **Review (2025-10-16, midday):** QUIC peer-cert caches now serialize through a sorted peer/provider view so `quic_peer_certs.json` stays byte-identical across runs and guard fixtures. New unit tests lock ordering, history, and rotation counters while `peer_cert_snapshot()` reuses the same sorted iterator to keep CLI/RPC payloads deterministic.
> **Review (2025-10-16, dawn+):** Light-client persistence, snapshots, and chunk serialization now run through deterministic first-party serializers with fixtures (`PERSISTED_STATE_FIXTURE`, `SNAPSHOT_FIXTURE`) and guard-parity tests covering account permutations plus compressed snapshot recovery. Integration coverage drives the in-house `coding::compressor_for("lz77-rle", 4)` path so resume flows remain identical under `FIRST_PARTY_ONLY`.
> **Review (2025-10-14, late evening+++):** Dependency governance now emits a
> structured summary and monitoring coverage. `dependency-check.summary.json`
> ships with every registry run, `tools/xtask` surfaces the parsed verdict,
> release provenance hashes the summary alongside telemetry/metrics artefacts,
> and regenerated Grafana dashboards plus alert rules visualise drift, policy
> status, and snapshot freshness. Integration/release tests enforce the artefact
> contract so CI and operations consume the same signals.
> **Review (2025-10-14, pre-dawn++):** Dependency governance automation now
> includes a reusable CLI runner that emits registry JSON, violations, telemetry
> manifests, and optional snapshots while returning `RunArtifacts` for automation
> and respecting a `TB_DEPENDENCY_REGISTRY_DOC_PATH` override. A new end-to-end
> CLI test drives that runner against the fixture workspace, validating JSON
> payloads, telemetry counters, snapshot output, and manifest listings. Registry
> parsing gained a complex metadata fixture covering optional/git/duplicate
> edges to lock adjacency deduplication and origin detection, and log rotation
> writes now roll back to the original ciphertext if any sled write fails
> mid-run.
> **Review (2025-10-14, late night+):** Dependency registry policy parsing and
> snapshotting run exclusively on the serialization facade. TOML configs flow
> through `foundation_serialization::toml::parse_table`, registry structs map to
> manual JSON `Value`s, and CLI outputs use `json::to_vec_value`, so serde drops
> from the crate while stub-mode tests stay enabled via new regression fixtures
> (including the TOML parser harness).
> **Review (2025-10-14, late night):** Log index key rotation now stages entries
> before writing so failures never leave mixed ciphertext; the suite added an
> atomic rotation regression test and the JSON probe exercises a `LogEntry`
> round-trip to skip cleanly when the stub backend is active. The dependency
> registry CLI drops `cargo_metadata`/`camino`, shells out to `cargo metadata`
> directly, and parses through the first-party JSON facade with unit and
> integration coverage that auto-skip on stub builds.
> **Review (2025-10-14):** `crates/sys` inlines the Linux inotify and BSD/macOS
> kqueue ABIs, removing the crate’s `libc` dependency while runtime’s
> `fs::watch` backend drops the `nix` bridge and reuses Mio registration for all
> platforms. Network sockets now rely solely on Mio helpers, finishing the
> `socket2` removal. Dependency inventories were refreshed to reflect the new
> first-party coverage.
> **Review (2025-10-12):** Runtime replaced the `crossbeam-deque` scheduler
> with a first-party `WorkQueue` that drives both async tasks and the blocking
> pool while preserving spawn-latency/pending-task telemetry. Added
> `crates/foundation_bigint/tests/arithmetic.rs` so addition, subtraction,
> multiplication, shifting, parsing, and modular exponentiation lock the new
> in-house big-integer engine against deterministic vectors across FIRST_PARTY
> and external configurations.
> **Review (2025-10-12):** Introduced the `foundation_serde` facade and stub
> backend, replicated serde’s trait surface (serializers, deserializers,
> visitors, primitive impls, and value helpers), and taught
> `foundation_serialization` to toggle between external and stub backends via
> features. Workspace manifests now alias `serde` to the facade, and
> `foundation_bigint` replaces the `num-bigint` stack inside `crypto_suite` so
> `FIRST_PARTY_ONLY` builds avoid crates.io big-integer crates; remaining
> `num-traits` edges live behind image/num-* helpers outside the crypto path.
> FIRST_PARTY_ONLY builds compile with
> `cargo check -p foundation_serialization --no-default-features --features
> serde-stub`, and the dependency inventory/guard snapshots were refreshed to
> capture the new facade boundaries.
> **Review (2025-10-14, midday++):** Dependency registry check mode now emits structured drift diagnostics and telemetry. The new `check` module stages additions, removals, field-level updates, root package churn, and policy deltas before persisting results via `output::write_check_telemetry`, guaranteeing operators can alert on `dependency_registry_check_status`/`dependency_registry_check_counts` even when rotation fails. `cli_end_to_end` gained a check-mode regression that validates the drift narrative and metrics snapshot, and synthetic metadata fixtures now cover target-gated dependencies plus default-member fallbacks so `compute_depths` stays correct across platform-specific graphs.
> **Update (2025-10-20):** Node runtime logging and governance webhooks now serialize via explicit first-party helpers. The CLI log sink assembles stderr JSON and Chrome trace output through `JsonMap` builders, and governance webhooks post typed payloads through the in-house HTTP client (`node/src/bin/node.rs`, `node/src/telemetry.rs`), removing the final `foundation_serialization::json!` dependency from production binaries.
> **Review (2025-10-12):** Delivered first-party PQ stubs (`crates/pqcrypto_dilithium`, `crates/pqcrypto_kyber`) so `quantum`/`pq` builds compile without crates.io dependencies while preserving deterministic signatures and encapsulations for commit–reveal, wallet, and governance flows. Replaced the external `serde_bytes` crate with `foundation_serialization::serde_bytes`, keeping `#[serde(with = "serde_bytes")]` annotations on exec/read-receipt payloads fully first party, and refreshed the dependency inventory accordingly. Runtime concurrency now routes `join_all`/`select2`/oneshot handling through the shared `crates/foundation_async` facade with a first-party `AtomicWaker`, eliminating the duplicate runtime channel implementation. New integration tests (`crates/foundation_async/tests/futures.rs`) exercise join ordering, select short-circuiting, panic capture, and oneshot cancellation so FIRST_PARTY_ONLY builds lean on in-house scheduling primitives with coverage.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, codec, serialization, SQLite, diagnostics, TUI, TLS, and the PQ facades are live with governance overrides enforced (2025-10-12). Log ingestion/search now rides the sled-backed `log_index` crate with optional SQLite migration for legacy archives (2025-10-14).

- [Progress Snapshot & Pillar Evidence](progress.md)
- [Dependency Sovereignty Pivot Plan](pivot_dependency_strategy.md)
- [Status, Roadmap & Milestones](roadmap.md)
 
> Consolidation note: Subsystem specs defer to `pivot_dependency_strategy.md` for wrapper-wide policy guidance so individual guides focus on implementation specifics.
- [Telemetry](telemetry.md)
- [Telemetry Operations Runbook](telemetry_ops.md)
- [Networking](networking.md)
  - [Overlay Abstraction](p2p_protocol.md#overlay-abstraction)
  - [Overlay Telemetry & CLI](networking.md#overlay-backend-troubleshooting)
  - [Storage Engine Abstraction](storage.md#0-storage-engine-abstraction)
- [Concurrency](concurrency.md)
- [Gossip](gossip.md)
- [Sharding](sharding.md)
- [Compute Market](compute_market.md)
- [Compute SNARKs](compute_snarks.md)
- [Monitoring & Aggregator Playbooks](monitoring.md)
- [Crypto Suite Migration](crypto_migration.md)
- [Codec & Serialization Guardrails](serialization.md)
- [HTLC Swaps](htlc_swaps.md)
- [Storage Market](storage_market.md)
- [Fee Market](fees.md)
- [VM Gas Model](vm.md)
- [Light Client Stream](light_client_stream.md)
- [Operators](operators/README.md)
  - [Run a Node](operators/run_a_node.md)
  - [Telemetry](operators/telemetry.md)
  - [Incident Playbook](operators/incident_playbook.md)
  - [Upgrade Guide](operators/upgrade.md)
- [Contributing](contributing.md)
- [Development Notes](development.md)
