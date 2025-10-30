# First-Party Dependency Migration Audit

> **2025-10-29 update (VRF committee guard, ANN proofs, pacing guidance):** The
> new `verifier_selection` crate implements VRF-backed committee sampling on top
> of the in-house `crypto_suite::vrf` module—VRF keys, outputs, and proofs are
> encoded with BLAKE3 transcripts and Ed25519 signatures so no external crypto
> crates are required. Selection receipts embed stake snapshots and committee
> receipts via the same first-party BLAKE3 hashing path, and the attestation
> manager’s guard now recomputes stake thresholds from the provided snapshot
> and cross-checks the serialized committee weights before accepting any wallet
> proof. Integration tests exercise stale snapshots, mismatched transcripts, and
> weight inflation through `ad_market.reserve_impression`, proving the guard
> strips invalid committees without leaning on third-party harnesses.
> Soft-intent ANN proofs derive encrypted buckets with
> `crypto_suite::encryption::symmetric` plus BLAKE3 keys, wallets can inject
> optional entropy that is persisted on receipts, and `badge::ann::verify_receipt`
> validates the mixed-entropy IV/ciphertext pair entirely on first-party crypto.
> The `ann_soft_intent_verification` benchmark now spans 128–32 768 bucket tables
> on the in-house `testkit`/`concurrency` stack, and gateway tests assert
> requested κ, multipliers, shadow prices, and ANN digests for every candidate so
> pacing analytics and receipt traces stay free of serde or external math crates.
> Benchmark runs publish `benchmark_ann_soft_intent_verification_seconds` plus
> the `_p50`, `_p90`, and `_p99` gauges, `benchmark_*_iterations`, and the
> new `benchmark_*_regression` flags through the first-party exporter under a
> file lock when `TB_BENCH_PROM_PATH` is set. History retention and alerting stay
> in-house: `TB_BENCH_HISTORY_PATH` + `TB_BENCH_HISTORY_LIMIT` append timestamped
> CSV rows while `TB_BENCH_REGRESSION_THRESHOLDS` + `TB_BENCH_ALERT_PATH` clamp
> acceptable runtimes and atomically record human-readable alerts without touching
> external tooling. Committee guard rejections likewise surface via
> `ad_verifier_committee_rejection_total{committee,reason}`, extending the
> existing telemetry with a pure first-party counter so operators catch stake or
> snapshot mismatches in real time.

> **2025-10-29 update (selection digest enforcement & pacing telemetry):**
> `SelectionReceipt::validate` now recomputes the commitment hash, proof-bytes
> digest, and verifying-key digest entirely through the manifest-backed
> `zkp::selection` helpers so tampered Groth16 payloads or stale keys are rejected
> without trusting wallet-side metadata. `extract_proof_body_digest` parses the
> proof envelope manually—no serde derives or external proof toolkits—and unit
> tests cover proof-bytes and verifying-key mismatches. Ad-market budget snapshots
> feed a first-party Prometheus bridge (`ad_budget_config_value`,
> `ad_budget_campaign_*`, `ad_budget_cohort_*`, `ad_budget_kappa_gradient`,
> `ad_budget_shadow_price`, `ad_resource_floor_component_usd`, and the
> `ad_budget_summary_value{metric}` series) that store label lifetimes via
> `HashSet` guards; the RPC handler records the snapshot once and telemetry fans
> out gauges with zero third-party metrics crates.

> **2025-10-29 update (privacy budgets, uplift, and dual-token ledger):** The new
> `PrivacyBudgetManager` enforces badge-family `(ε, δ)` ceilings solely with `std`
> collections and emits `ad_privacy_budget_{total,remaining}` via the existing
> `foundation_metrics` facade—no external DP libraries or serde derives. The
> cross-fitted `UpliftEstimator` drives `ad_uplift_propensity` and
> `ad_uplift_lift_ppm` using the same macros, threads its predictions through
> receipts/RPC helpers built on `foundation_serialization::json::Value`, and keeps
> calibration math on the in-house numeric helpers. Dual-token settlements gained a
> sled-backed `TokenRemainderLedger` serialised with manual JSON walkers; CT/IT
> remainders, TWAP window IDs, and composite floor breakdowns now round-trip
> without serde or third-party math crates, and persistence tests touch only
> first-party storage adapters.

> **2025-10-29 update (selection manifest determinism & pacing deltas):** Tests
> now cover manifest hot-swaps and multi-entry manifests entirely through the
> hand-rolled `parse_manifest_value`/`parse_artifacts_value` helpers, proving that
> circuit listings and verifying-key digests stay deterministic even when JSON
> order changes—no serde derives or external map iterators were introduced.
> `SelectionReceipt` gained an inline `resource_floor_breakdown` structure that
> serialises through the existing `foundation_serialization` facade and validates
> purely with `std` arithmetic, so wallets prove the full composite floor without
> relying on third-party math crates. The budget broker exposes
> `BudgetBrokerPacingDelta`/`merge_budget_snapshots` with plain `HashMap`
> bookkeeping; `node/src/rpc/ad_market.rs` and the telemetry tests assert the
> deltas align with the summary gauges using only the in-house Prometheus wrappers.

> **2025-10-28 update (selection proof metadata & broker snapshots):** Selection
> commitments now render entirely through handwritten
> `foundation_serialization::json::Value` maps—wallet receipts hash the raw
> commitment bytes with BLAKE3 and the attestation manager no longer relies on
> serde derives or macro helpers. The SNARK verifier gained manual envelope
> parsing, base64 decoding through the in-house `base64_fp` crate, and a
> registry of circuit descriptors behind `foundation_lazy::Lazy`, keeping proof
> validation hermetic while surfacing structured error reasons. Budget-broker
> snapshots serialise via bespoke JSON helpers shared by both marketplaces,
> persist to sled under `KEY_BUDGET`, and restore through the same path on
> startup, so κ shading, epoch spend, and dual-price state survive restarts
> without third-party storage or serde stubs. Selection receipts now embed
> proof metadata (circuit id, revision, digest, public inputs) assembled from
> the first-party verifier, and node/tests cover round-trips without pulling in
> external JSON tooling.

> **2025-10-28 update (selection attestation, budget broker, badge guard):** The
> ad marketplace now computes selection commitments with
> `foundation_serialization::json::to_vec` + BLAKE3 and verifies wallet proofs
> via the new `zkp::selection` module; SNARK verification, TEE fallbacks, and
> attestation metrics all ride through the in-house crypto + metrics facades—no
> third-party proof systems or logging crates were introduced. The pacing refactor
> adds a first-party `BudgetBroker` that tracks κ shading, campaign spend, and
> telemetry without pulling optimisation libraries, and the badge guard’s
> k-anonymity relaxations use only `std` collections. `ad_budget_progress`, the
> shadow-price gauges, κ gradients, and resource-floor breakdowns extend the
> existing Prometheus macros so dashboards keep consuming first-party telemetry.
> Missing attestations now respect
> `require_attestation` without panic instrumentation, keeping the matching path
> hermetic.
> **2025-11-09 update (dual-token governance gate & treasury timelines):** The
> new `DualTokenSettlementEnabled` parameter flows through governance and node
> codecs/stores via handwritten cursor helpers; no schema generators or serde
> derives were introduced while adding the toggle to sled-backed registries.
> Block codecs in `node/src/block_binary.rs` continue to rely on the in-house
> binary facade when persisting `treasury_events`, and explorer/CLI surfaces reuse
> the existing SQLite models plus manual JSON builders to expose the timelines.
> Metrics aggregator telemetry simply extends `telemetry_summary_to_value` with
> readiness deltas and adds the `AdReadinessUtilizationDelta` rule to the
> first-party Prometheus configuration, keeping CI/dashboards entirely on the
> in-house stack.

> **2025-11-08 update (readiness telemetry utilisation maps):** The node persists
> both the archived oracle snapshot and the live marketplace oracle in
> `AdReadinessSnapshot`, records per-cohort target/observed/delta utilisation, and
> publishes a ppm summary without introducing serde derives or external stores.
> `telemetry::summary` exposes the same fields via the in-house Prometheus facade,
> and `metrics-aggregator` registers
> `ad_readiness_utilization_{observed,target,delta}_ppm` using the existing
> registry wrappers while pruning stale label handles with a `HashSet`. Alerting
> and CI artefacts now consume the richer readiness map without importing third-
> party metrics crates or HTTP clients.
> **2025-10-28 update (treasury dual-token telemetry & audit filters):** The
> governance, node, explorer, CLI, and metrics-aggregator crates gained
> `amount_it`, `balance_it`, and `delta_it` coverage entirely through the
> existing handwritten cursor and JSON builders. Treasury disbursement filters for
> amount/timestamp bounds extend the same request structs without pulling a query
> DSL, and CLI flags simply map to the manual JSON envelope. The aggregator’s new
> `treasury_disbursement_amount_{ct,it}` and
> `treasury_balance_{current,last_delta}_{ct,it}` gauges reuse the in-house
> Prometheus facade and reset helpers, while the alert validator’s expanded
> dataset keeps the readiness delta rules hermetic. No third-party schedulers,
> query builders, or metrics crates were introduced while wiring the dual-token
> path end to end.
> **2025-11-07 update (ad market dual-token settlement):** The
> `ad_market` crate now computes USD→CT/IT token splits entirely with existing
> arithmetic helpers and sled/state locks; no new crates were introduced while
> extending `SettlementBreakdown` to expose oracle snapshots, CT totals, mirrored
> IT quantities, and residual USD. The follow-up liquidity fix reuses the same
> helpers to apply `liquidity_split_ct_ppm` before minting CT, destructures the
> per-role map so the CT path never sees the unsplit liquidity bucket, and layers
> debug assertions that validate each minted slice against its USD budget. The
> refreshed unit coverage asserts the split and now includes an uneven-price,
> pure-liquidity regression so future migrations can rely on the first-party
> logic.
> **2025-10-27 update (object-store signing & monitoring codecs):** The
> `foundation_object_store` crate now ships a canonical-request regression and a
> blocking upload harness that verify AWS Signature V4 headers without ever
> shelling out to third-party SDKs. `sim/chaos_lab.rs` routes uploads through a
> retry helper that honours `TB_CHAOS_ARCHIVE_RETRIES` (min 1) and optional
> `TB_CHAOS_ARCHIVE_FIXED_TIME` timestamps so deterministic runs never leak into
> external tooling, and the new tests prove the signed headers match AWS’
> published example canonical request. Monitoring dropped the last serde derives
> in `monitoring/src/chaos.rs`, decoding readiness snapshots solely through
> handwritten `foundation_serialization::json::Value` walkers so the
> `foundation_serde` stub stays dormant during CI and production builds.

> **2025-10-27 update (chaos archive manifests & object-store crate):**
> `sim/chaos_lab.rs` now emits a run-scoped `manifest.json`, a `latest.json` pointer, and a deterministic `run_id.zip` bundle for every chaos rehearsal, recording byte lengths and BLAKE3 digests without leaning on serde derives or external archivers. Optional `--publish-dir`, `--publish-bucket`, and `--publish-prefix` flags mirror the manifests and bundle through the new `foundation_object_store` crate, which wraps the existing first-party HTTP/TLS client so uploads never depend on third-party SDKs. `tools/xtask chaos` consumes the manifests via manual `foundation_serialization::json::Value` helpers, surfaces publish targets alongside readiness analytics, and continues to gate releases on overlay regressions entirely within the in-tree tooling. `scripts/release_provenance.sh` refuses to continue unless `chaos/archive/latest.json` and the referenced manifest exist, and `scripts/verify_release.sh` parses the manifest to ensure every archived file is present and that the recorded bundle size matches the on-disk `run_id.zip`, keeping release provenance hermetic.

> **2025-10-27 update (chaos failover gating & CLI smoke tests):**
> `sim/chaos_lab.rs::provider_failover_reports` synthesises provider outages, writes `chaos_provider_failover.json`, and errors when a failover fails to drop readiness or produce a `/chaos/status` diff. `cargo xtask chaos` loads the diff and failover artefacts, blocks releases on overlay regressions (readiness drops, removed sites, or missing failover diffs), and logs per-scenario readiness transitions. `scripts/release_provenance.sh` now shells out to `cargo xtask chaos --out-dir releases/<tag>/chaos` before hashing build artefacts and refuses to continue when the gate fails, while `scripts/verify_release.sh` rejects archives missing the `chaos/` snapshot/diff/overlay/provider failover JSON quartet, keeping provenance checks first party. The ban CLI harness now includes success-path smoke tests for `ban`, `unban`, and `list` alongside the existing failure injections so first-party telemetry stays covered.

> **2025-10-27 update (chaos/status fetch & xtask overlay analytics):**
> `sim/chaos_lab.rs` now downloads `/chaos/status` baselines with the in-house
> `httpd::BlockingClient` and manually decodes the payload using
> `foundation_serialization::json::Value`, mapping fields through
> `SnapshotDecodeError` helpers so no serde derives or third-party HTTP stacks
> sneak back in. Baseline arrays and overlay readiness rows persist via `std::fs`
> and `foundation_serialization` writers, and the new decode path converts
> provider/site readiness records entirely with first-party enums. The emitted
> overlay JSON feeds `cargo xtask chaos`, which now parses it through the same
> facade and summarises modules, scenario readiness, provider churn, readiness
> regressions, and duplicate site detection using only `std` collections—no
> external CLIs or telemetry toolkits were introduced.

> **2025-10-27 update (provider-aware chaos & listener helper):** Provider
> metadata now lives entirely in `sim/src/chaos.rs` via the first-party
> `ChaosProviderKind` enum, and `SiteReadinessState` stores readiness plus
> provider without reaching for serde or external maps. The attestation pipeline
> threads that metadata through monitoring and the metrics aggregator, emitting
> `chaos_site_readiness{module,scenario,site,provider}` while removing stale
> handles with the existing Prometheus facade—no new metrics crates were added.
> `sim/chaos_lab.rs` writes provider-aware diff artefacts using `std::fs`, and
> `metrics-aggregator` deletes outdated label handles with the in-house
> `GaugeVec` wrapper. Listener startup now routes through
> `node/src/net/listener.rs`, which simply wraps `std::net::TcpListener`/`UdpSocket`
> and the existing `diagnostics` logging; RPC, gateway, status, and explorer
> servers emit `*_listener_bind_failed` warnings without introducing async
> runtimes or third-party bind helpers. The new `node/tests/rpc_bind.rs` and
> `explorer/tests/explorer_bind.rs` regressions use only the shipped test harness
> to assert the warnings surface when ports are occupied. The ban CLI now tests
> storage/list failures through the shared `BanStore` trait so negative paths
> keep bubbling errors via first-party logging instead of silently mutating
> metrics.

> **2025-10-26 update (mixed-provider chaos & runtime stubs):** Overlay chaos
> scenarios now declare weighted `ChaosSite` entries inside `sim/src/chaos.rs`,
> and the metrics aggregator surfaces those arrays via
> `chaos_site_readiness{module,scenario,site,provider}` while sorting and logging
> `chaos_status_tracker_poisoned_recovering` when recovering from a poisoned
> mutex—no third-party metrics or JSON crates were introduced. The mobile sync
> suite now ships a `measure_sync_latency` stub compiled whenever the
> `runtime-wrapper` feature is disabled, keeping CI/test builds entirely on
> first-party runtime helpers instead of depending on external clients, and the
> new `node/tests/net_start_bind.rs` integration test captures gossip warnings
> through the existing `diagnostics` subscriber without adding logging deps.
> **2025-12-14 update (WAN chaos attestations & aggregator gating):** The new
> `ChaosHarness` lives entirely in `sim/src/chaos.rs` and signs readiness drafts
> via the first-party `monitoring` crate—`monitoring/src/chaos.rs` defines the
> codecs, digest, and Ed25519 verification helpers without leaning on serde or
> external crypto. The metrics aggregator depends on `monitoring-build` to parse
> `/chaos/attest` payloads, stores snapshots in the existing in-house store, and
> publishes readiness gauges through the foundation metrics facade. CI invokes
> `just chaos-suite` and `cargo xtask chaos`, both of which execute the
> first-party `chaos_lab` binary; no third-party orchestration tooling or HTTP
> clients were introduced. Aggregator remediation tests now share a
> first-party dispatch-log guard so environment mutations run sequentially while
> remaining entirely on `std::sync` primitives. The metrics aggregator pulls in
> `tb-sim` as a dev-dependency solely for the new
> `chaos_lab_attestations_flow_through_status` regression, keeping the end-to-end
> verification loop first-party, and Grafana’s auto-generated dashboards continue
> to render via `monitoring/build.rs` without outside tooling while adding the
> dedicated **Chaos** row for readiness/breach panels. Follow-up negative
> coverage (`chaos_attestation_rejects_invalid_signature`) tampers with payloads
> using only first-party helpers, while the gossip relay and peer metrics cache
> degrade to in-process fallbacks instead of panicking—no external temp-dir or
> timing crates were pulled in to achieve the new resilience.

> **2025-10-25 update (read-ack privacy proofs & collision-free reservations):**
> Read acknowledgements now rely on the in-house `zkp` crate for readiness and
> identity commitments. Gateway and node wiring derives signature-salted identity
> hashes, attaches readiness proofs, and exposes `--ack-privacy` plus
> `node.{get,set}_ack_privacy` without introducing third-party crypto stacks. The
> ad marketplace hashes a per-ack discriminator so identical fetches no longer
> overwrite reservations, and integration tests cover both the proof flow and
> collision-free settlements end to end.
>
> **2025-10-26 update (read-ack fixture reuse & marketplace locking):** The
> `read_ack_privacy` regression replaced `once_cell`-style globals with the
> first-party `concurrency::Lazy` helper so readiness snapshots and signing keys
> build once per run—no external cells required—and still exercise tampering
> coverage. The ad marketplace now holds its pending-budget lock through
> reservation insertion for both in-memory and sled backends, preventing
> oversubscription without reaching for external synchronization crates; the new
> concurrent reservation test proves only funded campaigns admit reservations.
> **2025-10-26 update (slack-aware trust routing & fallback coverage):**
> `TrustLedger::find_best_path` now favours the path with the highest residual
> slack via a heap backed entirely by `std` collections, then reuses the existing
> Dijkstra search to surface a shortest-path fallback. No graph crates or
> optimisers were introduced. Integration tests exercising multi-batch fairness,
> challenged withdrawals, and hop-limited fallbacks run exclusively against the
> in-tree router, bridge, and trust-ledger modules.
>
> **2025-11-02 update (deterministic liquidity router & runtime lint debt):**
> The new `node/src/liquidity/router.rs` module orchestrates DEX escrows,
> bridge withdrawals, and trust-line rebalances using only existing first-party
> crates (`dex`, `bridge`, `TrustLedger`). Governance-configurable batch size,
> fairness jitter, hop caps, and rebalance thresholds ride through the shared
> config structs, and execution hands off to the in-tree escrow/bridge helpers—no
> external schedulers, crypto crates, or dependency graph changes. Runtime’s
> integration suites explicitly allow `clippy::unwrap_used`/`expect_used` in
> test modules and guard histogram bucket sorting against NaNs, clearing the
> outstanding lint debt so `cargo clippy --workspace --all-targets` runs cleanly
> without third-party suppressors.
> **2025-11-06 update (stake escrow RPCs & atomic settlement):** DNS stake
> management now rides entirely on handwritten JSON/binary helpers. The new
> `dns.register_stake`, `dns.withdraw_stake`, `dns.stake_status`, and
> `dns.cancel_sale` handlers operate on `SimpleDb` escrows plus the existing
> `BlockchainLedger` facade—no serde, RPC clients, or async runtimes needed.
> Escrow rows now capture per-transfer `ledger_events` (with `tx_ref`) using the
> same handwritten codecs, and the RPC/CLI responses simply reflect those
> records so explorers can reconcile deposits/withdrawals without layering on
> third-party serializers.
> Ledger settlement batches use the `DomainLedger::apply_batch` hook so bidder
> debits, seller proceeds, royalties, and treasury fees all land atomically while
> recording deterministic memo strings for history. CLI coverage reuses the
> first-party JSON builders and RPC client (`gateway domain stake-*`, `cancel`).
> Integration tests continue to run under the mocked ledger without sockets or
> third-party harnesses, now covering stake deposits/withdrawals and auction
> cancellation.
> **2025-11-05 update (ledger-settled auctions & readiness persistence):**
> Premium domain settlement stays entirely on the first-party ledger facade.
> `BlockchainLedger` debits/credits accounts with deterministic memo strings,
> the auction module records transaction references in sale history, and the
> integration tests exercise winner/loser flows against the mocked chain without
> external RPC clients or serialization crates. Stake enforcement reuses the
> existing `SimpleDb` escrow records with handwritten codecs, so no third-party
> staking helpers land. Ad readiness persistence now stores events via manual
> binary writers in a dedicated sled namespace; startup replay and pruning run
> through the same in-house helpers, keeping readiness thresholds durable across
> restarts without adding databases or async runtimes.
> **2025-11-06 update (dual-token explorer & readiness telemetry):** Explorer,
> CLI, metrics aggregator, and readiness RPC surfaces now expose industrial-token
> splits, USD totals, settlement counts, and oracle snapshots entirely through
> the existing manual codecs. The ledger/genesis writers gained CT+IT fields via
> handwritten binary cursors, explorer/CLI JSON builders render the same data
> without serde, the aggregator registers additional Prometheus families using
> the in-house registry wrappers, and `ad_market.readiness` reuses the
> foundation JSON helpers to embed both snapshot and live oracle data. No third
> party crates were linked to support the conversion math; CI artefacts and
> dashboards consume the same first-party gauges.
> **2025-11-03 update (ad readiness gating & domain auctions):** Readiness
> thresholds live entirely inside the existing node crates—`ad_readiness`
> exposes manual cursor codecs and shared handles, the gateway consults the
> handle before matching, telemetry/RPC plumbing reuse the foundation metrics
> facade, and the aggregator extends its recorder without pulling in Prometheus
> clients or async runtimes. Premium domain auctions likewise stay on
> first-party primitives: sled-backed `SimpleDb` buckets persist auctions,
> bids, ownership, and sale history through handwritten binary cursor writers;
> CLI commands build JSON payloads via the existing helpers; and regression
> suites use std threads + in-process harnesses. No serde/jsonrpc/HTTP
> dependencies were reintroduced.
> **2025-11-03 update (governance-synced ad splits & atomic reservations):**
> Wiring the governance runtime to the sled-backed marketplace kept everything
> inside existing crates—`governance`, `ad_market`, and the node runtime share
> a `MarketplaceHandle` without introducing new RPC clients or async runtimes.
> Distribution updates use manual JSON/binary builders already in place, and the
> new concurrency ledger in `ad_market` relies solely on `std` locks plus the
> existing sled trees. The multi-threaded regression harness exercises the
> in-process marketplace with pure std threads, so no third-party fuzzing or
> testing frameworks were added. Operator docs/RPC coverage reuse the same
> first-party helpers, keeping the migration story hermetic.

> **2025-11-02 update (bridge parser coverage & remediation legends):** The contract CLI now exercises the settlement-log and reward-accrual commands through the first-party parser, locking optional asset/relayer filters, cursor forwarding, and default page sizes without reviving serde helpers. Node restart coverage stays inside the sled-backed bridge crate via `bridge_pending_dispute_persists_across_restart`, proving challenged withdrawals remain available through `pending_withdrawals` and `bridge.dispute_audit` after a reopen. Monitoring adds `dashboards_include_bridge_remediation_legends_and_tooltips` to assert Grafana legends/descriptions remain first-party across every generated template—no external validators or UI harnesses required.

> **2025-11-01 update (bridge dispute audit defaults & Grafana guards):** The contract CLI now exercises `BridgeCmd::DisputeAudit` through the first-party parser to confirm the default page size and localhost RPC fallback survive option handling, and a new transport regression proves `asset=None`/`cursor=None` payloads serialise to JSON `null` without reintroducing serde helpers. Monitoring’s `dashboards_include_bridge_counter_panels` test walks every generated Grafana JSON (dashboard/operator/telemetry/dev) to assert the reward-claim, approval, settlement, and dispute panels retain their first-party expressions and legends. These additions expand coverage while keeping every harness strictly in-house—no third-party transports, validators, or dashboard tooling required.

> **2025-10-31 update (spool sandbox & payout monotonicity):** Bridge remediation regressions now allocate a per-test `RemediationSpoolSandbox` that fabricates temporary directories via `sys::tempfile`, injects scoped `TB_REMEDIATION_*_DIRS` guards for page/throttle/quarantine/escalate, and includes a dedicated regression verifying the guards restore prior environment values—no manual `/tmp` cleanup, no external harnesses. Explorer payout ingestion clamps counter regressions inside `metrics_aggregator::record_explorer_payout_metric`, logging trace-only diagnostics, and the new churn-focused plus peer-isolation regressions keep coverage entirely on the first-party HTTPd router while alternating read/advertising role labels; no third-party metrics helpers or JSON tooling were introduced.
> **2025-10-24 update (explorer payout ingestion & Prometheus counter caching):** Explorer integration coverage now drives the mixed binary/JSON payout path entirely through the in-house codecs, and the metrics aggregator rewrites its recorder to cache role totals per peer while updating the existing `CounterVec` handles directly. Prometheus assertions in the integration suite validate the counters without leaning on external exporters, and documentation captures the CLI automation examples using only first-party tooling.
> **2025-10-30 update (payout fallback coverage & dashboard row):** Explorer payout queries now exercise the JSON fallback with legacy snapshots entirely inside the existing SQLite/`foundation_serialization` stack, the CLI’s error-path tests ride the mock transport without HTTP shims, and the Grafana generator renders the new “Block Payouts” row through the in-house dashboard builder. No third-party codecs, JSON libraries, or charting packages were introduced—the updates expand test coverage and templates while staying within the workspace crates.

> **2025-10-29 update (explorer/CLI payout surfaces & ack incident playbook):** Explorer `/blocks/:hash/payouts` and `contract-cli explorer block-payouts` reuse the existing first-party SQLite + JSON codecs to render per-role read/ad totals with no external serialization helpers, and the node integration test proving mixed flows mines blocks through the in-house ledger. Governance/monitoring docs covering the `read_ack_processed_total{result="invalid_signature"}` response loop are pure documentation updates—no third-party alert tooling or SDKs were introduced.

> **2025-10-29 update (read subsidy split & ad marketplace):** The new `ad_market`
> crate, acknowledgement worker, and block subsidy split land entirely on
> first-party crates. Campaign matching, settlement breakdowns, and the
> `read_sub_*_ct`/`ad_*_ct` totals are persisted via manual binary/JSON builders
> without introducing third-party SDKs. Mobile cache persistence now uses the
> binary cursor codec so integration tests run under the stub backend without
> serde panics, and telemetry (`read_ack_processed_total{result}`) comes from the
> in-house metrics facade. Explorer/dashboard follow-ups should reuse the same
> first-party fields—no external analytics libraries are required.
> **2025-10-28 update (gateway ack signing):** Gateway reads now require the
> first-party signature bundle supplied via the `X-TheBlock-Ack-*` headers. The
> server derives the client hash locally, verifies the Ed25519 signature before
> enqueueing a `ReadAck`, and the refreshed docs/tests cover the contract without
> introducing any external crypto or HTTP tooling.
> **2025-10-27 update (spool persistence & dashboard guard):** Bridge
> remediation spool artefacts now persist across acknowledgement retries and are
> drained automatically once hooks acknowledge or close an action. The restart
> suite exercises the cleanup path, the contract remediation CLI surfaces each
> action’s `spool_artifacts` in filtered and JSON views, and monitoring gained
> regressions that verify both the latency overlays and the new
> `bridge_remediation_spool_artifacts` gauge/panel remain wired into Grafana—all
> without introducing third-party tooling.

> **2025-10-27 update (ack targets & CLI filters):** Bridge remediation dashboards
> now overlay the first-party gauge
> `bridge_remediation_ack_target_seconds{playbook,policy}` on the latency
> histogram, the metrics aggregator rehydrates the histogram state after
> restarts, and Prometheus raises `BridgeRemediationAckLatencyHigh` when p95
> acknowledgements exceed the configured policy target. The contract CLI’s
> `contract remediation bridge` command added `--playbook`, `--peer`, and
> `--json` options so responders and automation filter or stream persisted
> actions without introducing third-party tooling.

_Last updated: 2025-11-02 09:00:00Z_

> **2025-10-25 update (remediation auto-retry & text acknowledgements):** The
> remediation engine now escalates and retries pending playbooks using only the
> in-house scheduler. Pending actions track `dispatch_attempts`,
> `auto_retry_count`, retry timestamps, and per-action `follow_up_notes` so the
> aggregator emits deterministic retry/escalation payloads without third-party
> queues. The acknowledgement parser tolerates plain-text hook responses
> (`"ack ..."`, `"closed: pager"`, etc.) alongside JSON objects, promoting each to
> a first-party `BridgeDispatchAckRecord` with persisted acknowledgement/closure
> metadata. Bridge alerts now query the stored acknowledgement counter to warn on
> pending or missing closures, keeping paging/escalation coverage entirely first
> party.

> **2025-10-25 update (dispatch acknowledgement telemetry):** The metrics
> aggregator now records `bridge_remediation_dispatch_ack_total{action,playbook,target,state}`
> alongside the existing dispatch counter, persists `acknowledged_at`/`closed_out_at`
> timestamps and notes on each remediation action, and the CLI/aggregator tests
> drive acknowledgement paths through a first-party HTTP override harness—no
> external servers required. Grafana/HTML dashboards chart acknowledgement
> deltas next to dispatch totals so the entire governance loop remains first
> party.
> **2025-10-25 update (remediation annotations & dispatch log):** Bridge remediation
> payloads remain fully hand-built and now embed operator-facing `annotation`
> strings, curated `dashboard_panels`, a deterministic `response_sequence`, and the
> canonical dispatch endpoint. Every attempt is captured in the in-memory
> `/remediation/bridge/dispatches` log with per-target status so paging and
> governance automation stay on the first-party stack—no third-party job queues
> or webhook proxies needed. CLI/tests assert these fields via the in-memory
> transports, locking the richer payloads to first-party serialization.
> **2025-10-24 update (dispatch health & dashboards):** The metrics aggregator now
> emits `bridge_remediation_dispatch_total{action,playbook,target,status}` for
> every HTTP/spool attempt, the CLI integration suite covers success/failure/
> skipped scenarios with first-party transports, and Grafana/HTML dashboards add
> a dispatch panel plus updated runbooks for operator triage.
> **2025-10-24 update (remediation dispatch & validator fixtures):** The metrics
> aggregator now fans remediation actions out to first-party HTTP endpoints or
> spool directories via the `TB_REMEDIATION_*_URLS`/`*_DIRS` environment
> variables, logging every dispatch and persisting JSON payloads without
> third-party queues. Operator runbooks document the matching liquidity response.
> The shared alert validator picked up recovery-curve and partial-window
> datasets so the bridge heuristics stay covered under FIRST_PARTY_ONLY.

> **2025-10-24 update (bridge remediation & multi-group validator):** The
> metrics aggregator now includes a first-party remediation engine that persists
> per-relayer actions, serves `/remediation/bridge`, and emits
> `bridge_remediation_action_total{action,playbook}` so incident tooling can page,
> throttle, or escalate without relying on external automation. The dedicated
> sled column family keeps the remediation baselines across restarts. The
> `monitoring` crate’s validator was generalised into
> `monitoring/src/alert_validator.rs`; the existing
> `bridge-alert-validator` binary now runs the shared helper to replay canned
> datasets for the bridge, chain-health, dependency-registry, and treasury alert
> groups, keeping Prometheus expressions hermetic without promtool.

> **2025-10-23 update (bridge skew alerts & validator):** Bridge alerting now
> ships per-label Prometheus rules (`BridgeCounterDeltaLabelSkew`,
> `BridgeCounterRateLabelSkew`) that remain entirely first party. The
> `monitoring` crate gained a validator binary that parses
> `monitoring/alert.rules.yml`, normalises the bridge expressions, and exercises
> canned datasets so label-specific regressions are caught without promtool. CI
> invokes the validator alongside the existing monitoring tests, keeping the
> alert group hermetic.
> **2025-10-23 update (settlement digest & reward accrual ledger):** External
> settlement proofs now compute a deterministic digest via the first-party
> `bridge_types::settlement_proof_digest` helper, track per-chain height
> watermarks inside `node/src/bridge/mod.rs`, and expose typed error variants for
> hash or height replays. Every duty success records a sled-backed
> `RewardAccrualRecord` retrieved through `bridge.reward_accruals`/
> `blockctl bridge reward-accruals`, with CLI/Node integration coverage ensuring
> pagination and JSON-RPC envelopes stay within the in-house helpers.
> **2025-10-23 update (CLI bridge transport abstraction):** Bridge commands in
> the contract CLI now route every JSON-RPC call through a new
> `BridgeRpcTransport` trait. Production flows wrap the existing `RpcClient`,
> while the integration suite injects an in-memory `MockTransport` that records
> envelopes and returns pre-seeded responses. This deletes the `JsonRpcMock`
> HTTP harness, drops the `runtime` executor from the test surface, and keeps
> FIRST_PARTY_ONLY builds hermetic without background servers.

> **2025-10-23 update (bridge reward claims & settlement proofs):** Governance
> reward approvals now persist through first-party sled helpers in both the
> governance crate and node mirror. `bridge.claim_rewards`, `bridge.reward_claims`,
> `bridge.submit_settlement`, `bridge.settlement_log`, and `bridge.dispute_audit`
> all assemble payloads via handwritten JSON builders, and the CLI mirrors the
> same approach for `blockctl bridge claim`, `reward-claims`, `settlement`,
> `settlement-log`, and `dispute-audit`. Cursor/limit pagination now ships across
> these RPC/CLI surfaces with `next_cursor` propagation so FIRST_PARTY_ONLY flows
> can stream long histories without serde helpers. Channel configuration updates
> accept optional fields without overwriting existing settings, and new unit
> tests in `governance/src/store.rs` plus `node/src/governance/store.rs` confirm
> reward approvals survive reopen and reject mismatched relayers. Telemetry now
> exports `BRIDGE_REWARD_CLAIMS_TOTAL`, `BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL`,
> `BRIDGE_SETTLEMENT_RESULTS_TOTAL{result,reason}`, and
> `BRIDGE_DISPUTE_OUTCOMES_TOTAL{kind,outcome}` alongside the existing challenge
> and slash counters. The integration suite (`node/tests/bridge_incentives.rs`)
> now covers reward redemption, settlement proofs, dispute audits, pagination, and
> telemetry increments end-to-end under FIRST_PARTY_ONLY. The contract CLI suite
> adds a `BridgeCmd::DisputeAudit` regression that drives the in-memory
> `MockTransport`, and the monitoring templates ship dedicated bridge panels so
> first-party dashboards chart the new counters without third-party widgets.

> **2025-10-22 update (bridge incentive ledger):** Bridge state persistence no
longer touches the `foundation_serde` stub. Incentive parameters and duty
records moved into a shared `bridge-types` crate, `node/src/bridge/mod.rs`
manually encodes/decodes the sled snapshots, and new RPC/CLI surfaces expose
`bridge.relayer_accounting`/`bridge.duty_log` alongside `blockctl bridge
accounting` and `blockctl bridge duties`. Integration suites
(`node/tests/bridge.rs`, `node/tests/bridge_incentives.rs`) now run under
FIRST_PARTY_ONLY without serialization panics while exercising reward, slash,
and governance override flows end to end.

> **2025-10-22 update (wallet signer metadata integration coverage):** The CLI
> wallet tests now assert the `signer_metadata` vector end-to-end. The
> `fee_floor_warning` suite verifies the struct-level metadata for ready and
> override previews, and a dedicated `wallet_signer_metadata` module snapshots
> local, ephemeral, and session entries while checking the auto-bump telemetry
> event—using only first-party `JsonMap` builders—so FIRST_PARTY_ONLY runs no
> longer depend on mock RPC clients to validate the new JSON surface.
> **2025-10-22 update (wallet signer metadata + CLI request tests):** `BuildTxReport`
> now exposes a `signer_metadata` field, and the wallet preview suite asserts on
> the JSON emitted across auto-bump, confirmation, ephemeral, and session flows,
> snapshotting the metadata array so FIRST_PARTY_ONLY runs exercise the same
> deterministic structure the CLI prints in JSON mode. Service-badge and telemetry modules gained helper-backed unit
> tests that snapshot the JSON-RPC envelopes for `service_badge.verify`/`issue`/`revoke`
> and `telemetry.configure`, keeping the CLI regression coverage on the
> first-party facade without mock servers or serde conversions. The mobile
> notification and node difficulty examples have been manualized as well,
> replacing their `foundation_serialization::json!` usage with explicit
> `JsonMap` builders so documentation tooling mirrors the production JSON
> pipeline.

> **2025-10-21 update (treasury helpers + CLI regression coverage):** Treasury
> CLI lifecycle and fetch tests now exercise the manual builders directly.
> `GovStore::record_treasury_accrual` funds disbursement executions, typed
> status assertions replace serde-based snapshots, and remote fetch tests cover
> `combine_treasury_fetch_results` with/without history, removing the last
> `JsonRpcMock` dependency and `foundation_serialization::json::to_value`
> conversions from the suite. FIRST_PARTY_ONLY runs no longer touch the serde
> stub during CLI testing, and green `cargo test` runs were captured for both the
> CLI and node crates.
> **2025-10-21 update (CLI JSON helpers + wallet manualization):** A new
> `json_helpers` module centralizes string/number/null constructors and
> JSON-RPC envelope helpers for the contract CLI. Compute, service-badge,
> scheduler, telemetry, identity, config, bridge, and TLS commands now build
> payloads through explicit `JsonMap` assembly instead of `foundation_serialization::json!`
> macros, while governance disbursement listings serialize through a typed view
> rather than an ad-hoc literal. Node-side surfaces follow suit: the runtime log
> sink constructs its map manually, and the staking/escrow wallet binary emits
> requests via the shared envelope helper, removing the last macro-based JSON
> construction from production binaries and keeping FIRST_PARTY_ONLY builds
> deterministic on operator tooling paths.
> **2025-10-21 update (webhook + CLI RPC builders):** Governance webhook
> delivery no longer depends on the `telemetry` feature flag—the node always
> posts to `GOV_WEBHOOK_URL` through the first-party HTTP client when the
> environment variable is configured, restoring notifications on minimal builds.
> The CLI’s networking surfaces (`contract net`, `gateway mobile-cache`,
> light-client status, and wallet send) replaced every
> `foundation_serialization::json!` literal with explicit `JsonMap` builders and a
> reusable `RpcRequest` envelope, keeping JSON-RPC bodies on the in-house facade
> and eliminating serde-backed macro usage along those paths. The node’s `net`
> binary mirrors the change for peer stats, exports, and throttle helpers so
> operator tooling stays FIRST_PARTY_ONLY end to end.

> **2025-10-20 update (admission tip + Kalman retune):** Transaction admission
> now derives `tx.tip` from `payload.fee` when callers omit a priority fee,
> keeping legacy builders compatible with the lane minimum and letting the
> base-fee regression run under FIRST_PARTY_ONLY without touching the
> `foundation_serde` stub. Inflation retuning replaced its serde-derived
> `KalmanState` serializer with manual `json::Value` parsing/encoding so the
> industrial multiplier history persists purely through first-party builders.
> **2025-10-20 update (transaction canonical bytes):** `canonical_payload_bytes`
> now forwards to `transaction::binary::encode_raw_payload`,
> `verify_signed_tx` hashes signed transactions via the manual writer, Python
> helpers decode through `decode_raw_payload`, and the CLI converts its payload
> struct into the node type before invoking the same encoder. This removes the
> last runtime dependency on `codec::serialize` for RawTxPayload/SignedTransaction
> and eliminates the `foundation_serde` stub panic that previously blocked the
> base-fee integration test under FIRST_PARTY_ONLY.
> **2025-10-20 update (RPC compute-market + DEX builders):** Compute-market
> responders (`scheduler_stats`, `job_requirements`, `provider_hardware`, and the
> settlement audit log) now emit JSON through deterministic first-party builders
> instead of `json::to_value`, keeping capability snapshots and audit rows on the
> in-house facade. DEX escrow status/release handlers convert payment proofs and
> Merkle roots via manual map assembly, removing the serde-based escape hatch
> while preserving the legacy array layout. Fresh unit coverage exercises the
> sorted drop/handshake maps that back the peer metrics RPCs so ordering stays
> deterministic.
> **2025-10-20 update (cursor field automation + peer stats JSON):** Block,
> transaction, and gossip encoders now build structs through
> `StructWriter::write_struct`, while the cursor exposes `field_u8`/`field_u32`
> shorthands so codecs enumerate layout metadata without closure plumbing.
> Round-trip tests cover the refreshed writers to guard against
> `Cursor(UnexpectedEof)` regressions. RPC peer metrics dropped
> `foundation_serialization::json::to_value` in favour of deterministic
> first-party map builders, keeping `net.peer_stats_export_all` on the in-house
> JSON stack and removing the last serde-based escape hatch from the networking
> export path.
> **2025-10-20 update (ledger persistence + mempool rebuild):** The new
> `ledger_binary` helpers now drive every on-disk snapshot—`MempoolEntryDisk`
> carries a cached `serialized_size`, the rebuild path consumes that byte count
> before re-encoding, and fresh unit tests cover `decode_block_vec`,
> `decode_account_map_bytes`, `decode_emission_tuple`, and legacy mempool entries
> without touching `binary_codec`. This locks ledger persistence and startup
> replay onto the cursor stack for FIRST_PARTY_ONLY runs.
> **2025-10-19 update (storage + networking tests):** Provider profile
> compatibility suites now construct their "legacy" fixtures through a dedicated
> cursor writer instead of `binary_codec::serialize`, locking the round-trip
> layout while keeping randomized EWMA/throughput coverage intact. Gossip peer
> telemetry tests likewise assert against the first-party JSON builders—unit
> tests and the aggregator failover harness both reuse `peer_snapshot_to_value`
> so no `foundation_serde` derives run during CI.

> **2025-10-19 update (network + ledger binaries):** Gossip messages, ledger
> blocks, and transactions now encode exclusively through first-party cursor
> helpers. `net::message` ships manual `encode_message`/`encode_payload`
> routines plus a comprehensive payload test suite (handshake, peer sets,
> transactions, blob chunks, blocks, chains, and reputation updates) so the
> networking stack no longer depends on the deprecated `binary_codec` shim.
> Ledger persistence introduces `transaction::binary` and `block_binary`
> modules that cover raw payloads, signed transactions (including quantum
> variants), blob transactions, and full blocks with cursor-backed encode/decode
> helpers and round-trip fixtures. Updated regression tests sort drop and
> handshake maps before asserting on encoded indices, keeping deterministic
> layouts aligned with the writers while the DEX/storage manifest suites inspect
> cursor output directly instead of the legacy codec.
> **2025-10-19 update (jurisdiction codec):** `crates/jurisdiction` now exposes
> first-party binary encoders/decoders for policy packs, signed packs, and typed
> diffs through the shared cursor helpers. CLI/RPC callers consume the new
> `PolicyDiff` struct instead of raw JSON blobs, while `persist_signed_pack` keeps
> JSON and `.bin` snapshots synchronized so sled-backed stores never rely on
> serde. Regression suites (`cargo test -p jurisdiction`,
> `tests/jurisdiction_dynamic.rs`) cover JSON, binary, and dual-format flows, and
> workspace callers can delete legacy `binary_codec` shims when migrating to the
> new helpers.
> **2025-10-18 update (treasury RPC + aggregator):** Governance RPC handlers now
> surface typed `gov.treasury.*` endpoints that decode through the
> `foundation_serialization` facade and share pagination helpers with the CLI.
> `contract gov treasury fetch` consumes those endpoints with first-party
> envelope parsing and emits actionable transport diagnostics, while the metrics
> aggregator reuses the sled-backed snapshots, tolerates legacy JSON records
> that stored numeric fields as strings, and warns when disbursements exist
> without matching balance history. The end-to-end HTTP integration test keeps
> the dispatcher on the first-party stack and guards the new RPC wiring.

> **2025-10-16 update (evening++)**: The serialization facade’s test suite now
> passes under the stub backend. `foundation_serialization::json!` supports
> nested objects, identifier keys, and trailing commas; every binary/JSON/TOML
> fixture ships handwritten serializers; and the `foundation_serde` stub adds
> direct primitive visitors (`visit_u8`/`visit_u16`/`visit_u32`) so tuple decoding
> works without the external derive stack. FIRST_PARTY_ONLY runs no longer skip
> serialization fixtures.
> **2025-10-14 update (closing push+++):** RPC fuzz harnesses now seed identity
> state through `sys::tempfile` scratch directories, letting FIRST_PARTY_ONLY
> runs avoid shared sled paths while the new smoke tests hit
> `run`/`run_with_response`/`run_request` directly. The sled legacy importer’s
> builder (`legacy::Config`) now drives migration, and fresh tests populate and
> reopen multi-tree manifests to lock the first-party JSON shim in place. The
> `tools/legacy_manifest` CLI gained deterministic column-family ordering and
> default-column coverage under integration tests, keeping the export story
> entirely in-house as we expand operator tooling.
> **2025-10-14 update (endgame)**: Net and gateway fuzz harnesses now reuse the
> shared `foundation_fuzz` modules, replacing `libfuzzer-sys`/`arbitrary`
> while `foundation_serde` and `foundation_qrcode` permanently drop their
> external-backend escape hatches. Remote-signer, the fuzz binaries, and
> every serialization call site now compile exclusively against first-party
> code paths, leaving the workspace lockfile free of crates.io entries.
> **2025-10-14 update:** The optional sled legacy importer now runs on a
> first-party manifest shim, so the workspace lockfile contains zero
> crates.io entries. `FIRST_PARTY_ONLY=1 cargo check` now succeeds for every
> crate—including all fuzz binaries—without needing feature gates.

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
| compute_market | `node/src/compute_market/price_board.rs` | 3 | manual `Serialize`/`Deserialize` + binary fixture | Price board persistence now relies on manual implementations that call the facade directly; `PRICE_BOARD_FIXTURE` locks the binary contract and the FIRST_PARTY_ONLY smoke test exercises encode/decode paths with the guard forced to `1`, `0`, and unset. |
| light_client | `crates/light-client/src/state_stream.rs` | 591-1030 | manual `Serialize`/`Deserialize` + deterministic ordering | Persisted-state, snapshot, and chunk payloads now use first-party serializers that sort account entries before encoding. `PERSISTED_STATE_FIXTURE` and `SNAPSHOT_FIXTURE` lock the bytes, while guard-on/off tests permute account orderings and cover compressed snapshot paths with `FIRST_PARTY_ONLY` forced to `1`, `0`, and unset. Randomized property tests now hammer compressed/uncompressed snapshot decoding and legacy `HashMap` fallbacks to keep the serializer/detector aligned across permutations. |
| compute_market | `node/src/compute_market/receipt.rs` | 3 | facade derive + optional defaults | Receipt encoding now references `foundation_serialization::defaults::default` and `foundation_serialization::skip::option_is_none`. |
| compute_market | `node/src/compute_market/scheduler.rs` | 3, 24-36, 849 | facade derive + defaults | Scheduler capability/reputation state uses facade helpers for defaults. |
| compute_market | `node/src/compute_market/settlement.rs` | 22, 62 | facade derive (`foundation_serialization::de::DeserializeOwned`) | Settlement pipeline routes SimpleDb blobs through the facade; optional fields use the facade skip helpers. |
| compute_market | `node/src/compute_market/workload.rs` | 1 | facade derive | Workload manifests serialize via the facade exports. |
| storage | `node/src/storage/fs.rs` | 6 | facade derive | Filesystem escrow entries serialize through the facade. |
| storage | `node/src/storage/manifest_binary.rs` | 1-420 | — (manual binary cursor encode/decode) | Object manifests, store receipts, chunk/provider tables, and sled receipts now encode via first-party cursor helpers with regression and legacy compatibility tests, plus a randomized property suite that hammers chunk/provider variants against the legacy codec. |
| storage | `node/src/storage/pipeline.rs` | 21, 53, 213-225 | facade derive + skip/defaults | Storage pipeline manifests use `foundation_serialization::{defaults, skip}` for optionals and collections; sled persistence defers to `pipeline/binary.rs`. |
| storage | `node/src/storage/pipeline/binary.rs` | 1-315 | — (manual binary cursor encode/decode) | Provider profile sled snapshots round-trip with cursor helpers and legacy parity tests, tolerating historical payloads that lacked the newer EWMA counters; the locked `PROVIDER_PROFILE_CURSOR_FIXTURE` plus FIRST_PARTY_ONLY smoke tests keep guard-on/guard-off builds byte-identical while the property harness randomizes EWMA/throughput fields to guard encoding parity. |
| storage | `node/src/storage/repair.rs` | 15, 139 | facade derive + rename_all | Repair queue tasks use facade derives with `rename_all`. |
| storage | `node/src/storage/types.rs` | 1, 19-58 | facade derive + defaults | Storage policy/state structures now reference facade defaults. |
| identity | `node/src/identity/did.rs` | 1-240 | — (manual binary cursor encode/decode) | DID registry sled persistence now routes through `identity::did_binary`, dropping `binary_codec` in favour of cursor helpers while preserving remote-attestation compatibility and replay detection. |
| identity | `node/src/identity/did_binary.rs` | 1-304 | — (manual binary cursor encode/decode) | Cursor helpers encode DID records, attestations, and optionals with the locked `DID_RECORD_FIXTURE` and FIRST_PARTY_ONLY smoke tests covering guard-on/guard-off parity; malformed-hash guards remain while the seeded property suite fuzzes randomized addresses/documents and the `identity_snapshot` integration test exercises mixed legacy/current sled dumps. |
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
| rpc | `node/src/rpc/client.rs` | 1-360 | facade wrappers + typed payload helpers | Client helpers now build envelopes via `Request::with_id/with_params` and decode responses through `foundation_rpc::ResponsePayload<T>`, removing bespoke JSON-RPC structs and keeping error propagation first-party. |

> **New first-party RPC facade:** the `foundation_rpc` crate now anchors the
> workspace-wide request/response schema, allowing `jsonrpc-core` to be removed
> from manifests while keeping CLI and runtime handlers on a shared, audited
> envelope.
> **2025-10-18 update (treasury + bridge RPC):** `governance::Params` now exposes
> `to_value`/`deserialize`, letting RPC handlers clone parameter envelopes through
> the facade instead of hand-rolled JSON maps. Bridge endpoints accept typed
> request/response structs, reuse a shared commitment decoder, and serialize every
> payload via `foundation_serialization::json`, eliminating the bespoke builders
> that previously lived in `node/src/rpc/bridge.rs`.
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

### Tooling & Support Crate Migrations (2025-10-16 update)

- ✅ The workspace `sled` crate now ships a first-party JSON manifest importer
  for the `legacy-format` feature, eliminating the crates.io `sled`
  dependency chain while preserving on-disk upgrade support.
- ✅ Net and gateway fuzz harnesses now mirror the shared
  `foundation_fuzz` modules, retire `libfuzzer-sys`/`arbitrary`, and ship
  smoke tests that exercise their entry points without libFuzzer glue.
- ✅ `crates/coding` dropped the `allow-third-party` feature flag; the LT fountain
  coder now encodes/decodes via the in-house Reed–Solomon engine and the
  property harness runs entirely on the workspace RNG (`crates/coding/tests/
  inhouse_props.rs`). `crates/rand` gained deterministic `fill` and
  slice-selection helpers with dedicated tests (`crates/rand/tests/seq.rs`), and
  simulation tooling (`sim/did.rs`) consumes the new APIs so account rotation
  stays first party.
- ✅ `foundation_serde` and `foundation_qrcode` permanently retired their
  external-backend features; every consumer (including the remote signer CLI)
  now relies on the in-house stubs so the workspace no longer references
  crates.io fallbacks even optionally.
- ✅ `tools/dependency_registry` exposes a reusable `run_cli` helper that writes
  registry JSON, violation reports, telemetry, manifest manifests, and optional
  snapshots while honouring a `TB_DEPENDENCY_REGISTRY_DOC_PATH` override for
  sandboxed runs. The function returns `RunArtifacts` so automation can inspect
  emitted paths without rehydrating the filesystem, and a new integration test
  exercises the full CLI flow against the fixture workspace. Parser coverage now
  includes a complex metadata fixture with optional/git/duplicate edges to lock
  adjacency deduplication and origin detection. Log archive key rotation gained
  a rollback guard so sled writes either complete fully or restore the original
  ciphertext when any storage error surfaces.
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
  new `foundation_windows` FFI crate declared in `crates/sys/Cargo.toml`,
  allowing `FIRST_PARTY_ONLY=1 cargo check --target x86_64-pc-windows-gnu` to
  pass for both `sys` and `runtime`. Remaining `mio` references live only behind legacy
  tokio consumers slated for follow-up migration.
- ✅ Mobile probes no longer depend on Objective-C or Android JNI bindings.
  `crates/light-client/src/device/ios.rs` now issues Objective-C messages and
  CoreFoundation queries through dedicated FFI helpers, removing the
  `objc`, `objc-foundation`, `objc_id`, and `core-foundation` crates while
  keeping battery monitoring and Wi-Fi checks intact. On Android, the probe
  delegates to new `sys::device::{battery,network}` modules that read
  `/sys/class/power_supply` and `/proc/net/wireless`, eliminating the `jni`,
  `ndk`, and `ndk-context` stacks. The shared helpers expose
  `battery::capacity_percent`/`is_charging` and `network::wifi_connected` so
  future CLI or runtime code can reuse the same first-party telemetry.
- ✅ Terminal prompting now has first-party coverage: `sys::tty` exposes a
  generic passphrase reader that unit tests exercise with in-memory streams,
  `foundation_tui::prompt` adds override hooks so downstream crates can inject
  scripted responses, and the CLI `logs` helpers now include unit tests that
  validate optional/required prompting without depending on external crates.
  Together they keep FIRST_PARTY_ONLY builds interactive-friendly while
  ensuring prompt behaviour is regression-tested.
- ✅ `tools/dependency_registry` now shells out to `cargo metadata` through a
  first-party parser layered on `foundation_serialization::json`, removing the
  crates.io `cargo_metadata` and `camino` crates. The registry builder stages
  metadata JSON through the facade, unit-tests the parser, and teaches the
  integration suite to skip automatically when the stub backend is active so
  FIRST_PARTY_ONLY runs stay green while policy enforcement continues to
  operate on in-house code.
- ✅ Policy loading and registry snapshots no longer depend on serde derives.
  `foundation_serialization::toml::parse_table` exposes the raw TOML document,
  `tools/dependency_registry::config` normalises tiers/licenses with manual
  validation, and the registry/model/output layers convert structs to and from
  `foundation_serialization::json::Value`. The CLI now writes snapshots and
  violations via `json::to_vec_value`, test fixtures run under the stub backend
  without skipping, and the crate’s `Cargo.toml` drops the workspace `serde`
  dependency entirely.

### Tooling & Support Crate Migrations (2025-10-12)

- ✅ Added the `foundation_serde` facade crate with a fully enumerated stub
  backend. The stub mirrors serde’s `ser`/`de` traits, visitor hierarchy,
  primitive implementations, and value helpers so FIRST_PARTY_ONLY builds can
  compile end-to-end. `foundation_serialization` now toggles backends via
  features (`serde-external`, `serde-stub`) without ever depending on upstream
  `serde` directly, and the stub backend passes `cargo check -p
  foundation_serialization --no-default-features --features serde-stub`. The
  stub has since grown direct `visit_u8`/`visit_u16`/`visit_u32` hooks so tuple
  decoding works without falling back to `visit_u64`.
- ✅ `crates/jurisdiction` now signs, fetches, and diffs policy packs via
  handwritten `foundation_serialization::json` conversions
  (`PolicyPack::from_json_value`, `SignedPack::from_json_slice`,
  `to_json_value`) and the in-house HTTP client, eliminating the `ureq` and
  `log` dependencies entirely. Law-enforcement logging emits
  `diagnostics::log` info records when appends succeed, and the refreshed test
  suite covers array/base64 signatures plus malformed pack rejection so the
  manual paths stay hardened.
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
  messages with the same facade. The macro now handles nested literals and
  identifier keys with regression coverage mirroring serde_json.
- ✅ Replaced the ad-hoc SQLite log tooling with the sled-backed
  `log_index` crate. CLI, node, explorer, and telemetry utilities now share the
  first-party store for ingestion, search, and key rotation, while the
  optional `sqlite-migration` feature only gates legacy imports through the
  `foundation_sqlite` facade. Diagnostics dropped the facade entirely once its
  emitters moved to pure telemetry, so FIRST_PARTY_ONLY builds no longer link
  SQLite shims.
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
- ✅ Expanded the `foundation_serde` stub backend with first-class coverage for
  primitives, options, tuples, collections, and enum variants so CLI and node
  crates compile cleanly under `FIRST_PARTY_ONLY=1`. The derive macros now
  pattern-match on structures to mark every field as used, and a compile test
  (`crates/foundation_serde/tests/deny_warnings.rs`) runs with `#![deny(warnings)]`
  to guarantee the stub keeps pace with production derives.
- ✅ Replaced the CLI’s `rpassword` prompt with the in-house
  `foundation_tui::prompt` module backed by cross-platform `sys::tty`
  primitives. Passphrase prompts now disable terminal echo via first-party
  termios/console bindings, eliminating the final third-party input dependency
  in the log tooling and keeping rotation/search flows usable in
  FIRST_PARTY_ONLY builds.
- ✅ `foundation_serde`’s stub backend now mirrors serde’s option/sequence/map/
  tuple/array coverage, and `foundation_serialization::json::Value` has manual
  serde parity. The CLI’s TLS warning/status/certificate structs were rewritten
  with handwritten serializers/deserializers so the TLS convert/stage/status
  flows no longer depend on derive macros, and
  `FIRST_PARTY_ONLY=0 cargo test -p contract-cli --lib` now runs entirely on the
  stub backend.
- ✅ Regression coverage now guards those manual codecs: `cli/src/tls.rs` includes
  JSON round-trip tests for warning status/snapshot/report payloads, the
  `crates/foundation_serialization/tests/json_value.rs` suite exercises nested
  structures, duplicate keys, and non-finite float rejection, and
  `node/src/storage/pipeline/binary.rs` gained
  `write_field_count_rejects_overflow` to prove the encoder’s guard path fires.
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

FIRST_PARTY_ONLY is now enforced across the workspace. Ongoing maintenance
focuses on guarding this posture: new tooling must route through the
`foundation_serialization` and telemetry facades, inventory refreshes should
run whenever crates are added or removed, and CI keeps the guard binary wired
to prevent accidental third-party reintroductions.

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

## 3. Guardrail & Maintenance Work

1. **Harden facade ergonomics:** continue upstreaming bespoke predicates (for
   example non-zero numeric guards) into `foundation_serialization` so callers
   never reintroduce ad-hoc helpers.
2. **Extend policy automation:** keep `tools/xtask` and CI workflows blocking on
   the dependency guard, publishing telemetry snapshots on every run so drift is
   visible in dashboards.
3. **Audit ecosystem hooks:** when SDKs or operator tooling branch from the
   workspace, verify their manifests re-export the in-house crates; publish
   checklists alongside the dependency registry exporter so downstream teams
   stay aligned with the first-party posture.
4. **Fixture Updates:** port test fixtures to use `crates/codec` (or new
   first-party binary encoders) and run both `FIRST_PARTY_ONLY=1 cargo test -p
   the_block` and `FIRST_PARTY_ONLY=0 cargo test -p the_block` after each
   migration stage.
5. **Math/FFT/Parallelism Replacement:** design in-house primitives under
   `crates/coding` or a new math crate to cover matrix algebra, chi-squared CDF,
   and DCT operations. Wire node/governance modules to the replacements and drop
   the third-party crates from manifests, then benchmark the new stacks.

## 4. Stub Backlog for FIRST_PARTY_ONLY Builds

The handle migration eliminated direct collector access across the node and
ancillary tooling, but several third-party crates still block
`FIRST_PARTY_ONLY=1` builds. The highest-impact items to stub are:

| Crate | Primary Consumers | Notes |
| --- | --- | --- |
| `rusqlite` | `cli`, `explorer`, `tools/{indexer,log_indexer_cli}` | ✅ Direct call-sites now route through the new `foundation_sqlite` facade. The facade persists via the in-house JSON helpers (`database_to_json`/`database_from_json`), eliminating the temporary binary encoder. Follow-up: migrate explorer/indexer import paths to the JSON snapshots and delete any residual `.db` bootstrap assets. |
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

- ✅ `foundation_rpc` still exposes typed envelope helpers, but the node RPC
  client now assembles request maps and parses responses manually through
  `foundation_serialization::json::Value`. This removes the remaining
  `foundation_serde` derive invocations from client-side payloads, keeps error
  reporting inside the first-party facade, and guarantees `FIRST_PARTY_ONLY`
  builds never touch the stub backend when issuing or decoding JSON-RPC calls.
- ✅ `foundation_serialization::json::Value` implements `Display`, restoring the
  `.to_string()` ergonomics expected by RPC callers while keeping output locked
  to the compact renderer via a new regression test.
- ✅ Wallet, light-client, and diagnostics surfaces now exclusively emit logs
  through `diagnostics::tracing`, removing the third-party `tracing` stack from
  the workspace manifests while preserving existing span/field semantics.
- ✅ Every fuzz harness (`fuzz`, `gateway/fuzz`, `net/fuzz`) now relies on the
  in-house `foundation_fuzz` crate (Unstructured reader + `fuzz_target!` macro),
  eliminating the `libfuzzer-sys`/`arbitrary` toolchain from the workspace and
  keeping FIRST_PARTY_ONLY builds feature-complete.
- ✅ `foundation_qrcode` always renders through the first-party backend; the
  optional crates.io `qrcode` feature flag was removed alongside the CLI toggle,
  dropping the `image`/`num-*` stack from the lockfile entirely.
- ✅ `foundation_serde` now exports only the first-party stub backend, retiring
  the external `serde` escape hatch and ensuring serialization derives always
  resolve to in-house implementations.
- ✅ Range sampling in `crates/rand` now uses rejection sampling so
  `u64`/`usize`/`i64` domains avoid modulo bias. New regression tests
  (`crates/rand/tests/range.rs`) cover tail-heavy spans, and the fountain
  property harness exercises parity-budget and burst-loss recovery. `tools/xtask`
  removed the `--allow-third-party` toggle, so dependency audits always run with
  `FIRST_PARTY_ONLY` enforcement.
- ✅ Dropped the dormant `static_assertions` crate from `node/Cargo.toml` and the
  first-party manifest, keeping compile-time checks on the standard library and
  shrinking the guard violation surface.

Windows bindings now ride `foundation_windows` for console/IOCP primitives, so
`FIRST_PARTY_ONLY=1` builds no longer flag the `sys` crate on Windows targets.
Follow-up work focuses on migrating remaining tooling consumers to the new FFI
facade and extending the crate with richer console abstractions.

- ✅ The dependency registry runner now emits `dependency-check.summary.json`
  alongside telemetry/violations, `tools/xtask` prints the parsed verdict during
  CI preflights, `scripts/release_provenance.sh` hashes the summary/telemetry in
  signed artefacts, and monitoring dashboards/alerts render the new dependency
  status metrics so drift is visible across automation and operations.
- ✅ Introduced `foundation_serialization::binary_cursor::{Writer, Reader}` and
  migrated gateway read receipts onto the manual first-party encoder/decoder,
  removing the last serde derive in that surface while preserving the legacy
  CBOR fallback path.
- ✅ Dependency registry check mode now stages drift summaries and emits
  `dependency-check.telemetry`, ensuring FIRST_PARTY_ONLY tooling surfaces
  detailed additions/removals/policy diffs even when the CLI aborts. A new
  integration test validates the narrative and metrics payloads, and fixtures
  now cover cfg-targeted dependencies plus default-member fallbacks to keep
  depth calculations accurate.
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
- ✅ Metrics aggregator ingestion now manualises telemetry summaries, TLS warning
  fingerprints, and treasury disbursement/balance snapshots through
  `foundation_serialization::json::Value` plus the governance codec helpers.
  The bridge anomaly detector exposes `/anomalies/bridge` JSON alongside the
  `bridge_anomaly_total` counter and emits
  `bridge_metric_delta{metric,peer,labels}`/
  `bridge_metric_rate_per_second{metric,peer,labels}` gauges so dashboards
  consume first-party payloads without serde derives or float-rounded
  fingerprints. Gauge baselines now persist in the in-house store and Prometheus
  alert rules (`BridgeCounterDeltaSkew`, `BridgeCounterRateSkew`) evaluate the
  gauges directly, keeping restart recovery and alerting inside the first-party
  stack.
- ✅ Manualized the node runtime log sink and governance webhook JSON builders
  (`node/src/bin/node.rs`, `node/src/telemetry.rs`) so production binaries no
  longer invoke the `foundation_serialization::json!` macro. Runtime logging,
  Chrome trace emission, and governance webhooks now serialize through explicit
  first-party `JsonMap` builders/typed structs, keeping admission and alerting
  surfaces compliant with the FIRST_PARTY_ONLY audit.
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
