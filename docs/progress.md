# Project Progress Snapshot
> **Review (2025-10-30, afternoon):** RangeBoost mesh delivery now includes a
> first-party queue forwarder that activates only when mesh mode is enabled,
> keeping HTTP-only gateways free of background workers while mesh deployments
> drain queued payloads and surface failures via `diagnostics::log`. Advertiser
> conversions require an `Authorization: Advertiser <account>:<token>` header
> paired with a campaign-level `conversion_token_hash` (BLAKE3 hex); the RPC
> rejects mismatched accounts, missing hashes, or bad tokens before recording the
> uplift observation, and integration tests cover success plus each failure path.
> `SettlementBreakdown` now emits the auction clearing price, delivery channel,
> and mesh payload digest/length alongside the dual-token CT/IT totals so
> diagnostics, sled snapshots, and dashboards all observe mesh deliveries. Regression
> coverage reran `cargo test -p ad_market`, `cargo test -p the_block --test
> ad_market_rpc`, and `cargo test -p the_block --test mesh_sim` to freeze the
> new flow end to end.
> **Review (2025-10-30, morning):** Ad targeting now spans geo, device, delivery,
> and advertiser CRM cohorts without leaving the first-party stack. Campaign and
> creative schemas in `crates/ad_market` gained manual JSON helpers plus mesh
> placement metadata, and the gateway threads `X-TheBlock-Geo-*`, device
> fingerprints, CRM registries, delivery-channel hints, and RangeBoost peer
> telemetry directly into `ImpressionContext`, selection traces, and
> `ReadAck`s. Holdout assignment from the uplift estimator rides through
> `SelectionReceipt`, reservation state, and read acknowledgements so clients can
> suppress control traffic deterministically. Conversion ingestion now flows
> end-to-end via the in-house RPC facade: the marketplace persists uplift
> snapshots across sled restarts, `ad_market.record_conversion` records
> treatment/control updates, and integration tests exercise both pathways
> alongside backward-compatible payloads. RangeBoost-enabled impressions enqueue
> mesh payloads with hop-proof scaffolding, keeping Bluetooth/Wi-Fi delivery on
> the same first-party queue that powers gateway peer discovery. Block codecs,
> RPCs, and genesis verification picked up the new ad-settlement fields (dual
> token IT payouts, treasury events, mesh payload metadata) and the consensus
> module now anchors genesis at `2fe62d67…`, with `node/build.rs` emitting the
> updated stub to keep CI/tooling in sync.
> **Review (2025-10-29, night):** VRF-backed verifier selection and encrypted
> badge soft intents now gate wallet proofs before settlement. Receipts carry
> stake snapshots, committee transcripts, and ANN proofs produced exclusively by
> the new `verifier_selection` crate and `badge::ann` helpers, while the
> attestation manager validates VRF outputs with first-party BLAKE3 hashing.
> The verifier guard now recomputes stake thresholds from the supplied
> snapshot—never from receipt weights—and integration coverage exercises stale
> snapshots, mismatched transcripts, and weight inflation end to end through
> `ad_market.reserve_impression`. Gateway and SDK surfaces emit requested κ,
> shading multipliers, ANN ciphertext digests, and dual-token toggles so analysts
> can trace selection guidance and badge intent in real time. The ANN pipeline
> now mixes wallet-provided entropy into key/IV derivation, stamps the entropy
> on receipts, and rejects tampered ciphertexts; gateway tests assert the new
> fields across every candidate in multi-creative traces so shading telemetry
> stays consistent beyond the winner. The `ann_soft_intent_verification`
> benchmark measures ANN receipt verification across 128–32 768 bucket tables
> with salted and wallet-entropy derivation, and the shared benchmarking harness
> locks the Prometheus export while writing `benchmark_ann_soft_intent_verification_seconds`
> alongside `_p50`/`_p90`/`_p99` percentiles so concurrent runs cannot clobber metrics.
> The monitoring generator ingests the updated samples and keeps ANN verification
> latency distributions plotted beside pacing telemetry without leaving the
> first-party stack.
> **Review (2025-10-29, evening):** Selection manifests now parse deterministically
> across hot swaps and multi-entry manifests via the manual
> `parse_manifest_value`/`parse_artifacts_value` helpers, with tests covering order
> permutations so verifying-key digests stay stable without serde. `SelectionReceipt`
> gained an in-band `resource_floor_breakdown`, validators enforce that the summed
> components cover the composite floor, and settlements persist the struct for
> dashboards and RPC consumers. The budget broker exposes
> `BudgetBrokerPacingDelta`/`merge_budget_snapshots`, telemetry asserts
> `ad_budget_summary_value{metric}` mirrors the RPC snapshot, and RPC tests verify
> partial updates merge before deltas are emitted—all using first-party
> collections and Prometheus wrappers.
> **Review (2025-10-29, late afternoon):** Privacy budgets, uplift estimation, and
> dual-token remainder accounting now ship in production form. The ad marketplace
> enforces badge-family privacy limits through `PrivacyBudgetManager`, returns
> cooling/denied decisions immediately, and emits
> `ad_privacy_budget_{total,remaining}` so telemetry and dashboards expose
> `(ε, δ)` consumption without third-party helpers. The new `UpliftEstimator`
> produces cross-fitted doubly-robust lift predictions, threads propensity,
> baseline action rate, sample size, and calibration error through candidate
> traces, reservations, receipts, RPC payloads, and telemetry
> (`ad_uplift_propensity`, `ad_uplift_lift_ppm`) so pacing optimises for lift rather
> than raw CTR. Settlements persist a `TokenRemainderLedger` that stores per-role
> CT/IT USD remainders alongside TWAP window IDs and exposes the composite
> `resource_floor_breakdown`; sled/in-memory marketplaces share the helper, RPC
> responses surface the new fields, and tests confirm remainders survive restarts
> and partial snapshot merges.
> **Review (2025-10-29, afternoon):** Selection proofs now authenticate the
> underlying Groth16 payload and verifying key—not just the wallet-supplied
> wrapper. `zkp::selection::extract_proof_body_digest` parses the proof envelope
> and hashes the raw bytes so `SelectionReceipt::validate` rejects mismatched
> `proof_bytes_digest` entries, while the metadata path cross-checks the
> manifest-backed verifying-key digest from `selection_artifacts.json`. Unit tests
> cover valid SNARK receipts plus tampered proof-bytes and verifying-key digests,
> and the attestation integration suite (`ad_market_attestation_budget_load`) now
> exercises SNARK validation, broker commit, and telemetry updates through the
> live RPC surface. Telemetry exports gained
> `ad_budget_summary_value{metric=campaign_count|cohort_count|mean_kappa|…}` so
> dashboards and governance tooling can track pacing analytics without
> reconstituting snapshots, and `ad_market.broker_state` exposes the generated-at
> micros plus the new `pacing` block alongside the existing campaign/cohort dump.
> **Review (2025-10-28, late night):** Budget pacing and proof plumbing now
> survive restarts. `BudgetBroker` snapshots serialise through first-party JSON
> helpers, persist under sled’s `KEY_BUDGET`, and restore during marketplace
> initialisation so κ shading, dual prices, and cohort spend carry across
> deployments. The crate now re-exports broker config and snapshot structs so the
> node RPC/CLI can surface pacing state without private-module shims. Selection
> receipts embed SNARK metadata (circuit id, revision,
> digest, public inputs) derived from the new `zkp::selection` verifier, and the
> attestation manager recomputes commitments through manual
> `foundation_serialization::json::Value` builders before hashing with BLAKE3.
> Verification now surfaces `ad_selection_proof_verify_seconds` latency and
> commitment gauges, while Grafana gained the advertising row plus
> `SelectionProofSnarkFallback`, `SelectionProofRejectionSpike`, and
> `AdBudgetProgressFlat` alerts. Unit tests cover budget snapshot round-trips,
> proof validation success/failure, and receipt metadata enforcement so the
> restart path stays hermetic.
> **Review (2025-10-28, evening):** Treasury consumers are now dual-token aware
> end to end. The metrics aggregator registers CT and IT gauges for disbursement
> totals, current balances, and last deltas, healing the partial refactor and
> wiring the new counters into reset paths so dashboards inherit accurate
> telemetry once governance flips the activation gate. Explorer and CLI surfaces
> expose matching history: the REST `/governance/treasury/disbursements`
> endpoint accepts status, amount, and timestamp filters with pagination cursors,
> and the CLI forwards `--min-amount-{ct,it}`, `--max-amount-{ct,it}`,
> `--min-created-at`, and epoch constraints to mirror long-range audits.
> Monitoring, CI artefacts, and dashboards now ingest the readiness delta map via
> the refreshed alert validator and rule deck, replacing legacy summaries with
> the new `ad_readiness_utilization_delta_ppm` series so runbooks page on cohort
> drift before activation.
> **Review (2025-10-28, late morning):** Dual-token settlement now rides a
> governance gate wired end to end. The new
> `DualTokenSettlementEnabled` parameter flows from the governance runtime to the
> node, flipping ad-marketplace distribution policies so CT-only clusters stay on
> legacy payouts while mixed deployments expose the CT/IT split. Block assembly
> now emits a per-block treasury execution timeline—reconciling executed
> disbursements against included transactions—and the explorer/CLI surfaces the
> same history so auditors can trace dual-token settlements alongside downstream
> treasury flows. Telemetry exporters push the richer readiness map (including
> `delta_utilization_ppm` and cohort summaries) through the metrics aggregator,
> and Prometheus gained an `AdReadinessUtilizationDelta` alert to page when
> cohorts drift from their targets despite steady demand.
> **Review (2025-11-08, evening):** Liquidity payouts now honour the
> `liquidity_split_ct_ppm` governance knob end to end. Both ad-marketplace
> backends convert the CT portion of the liquidity share before minting tokens,
> preventing the IT allocation from being double counted in CT totals, and the
> updated unit test asserts the expected split against deterministic oracle
> prices. The conversion helper now destructures the per-role USD map so the CT
> path never sees the unsplit liquidity bucket, and debug assertions enforce that
> each tokenised slice recombines with its rounding remainder to match the
> original USD split. A complementary rounding regression keeps the uneven-price
> case honest, exercising a pure-liquidity settlement where both CT and IT
> receive non-zero shares without ever exceeding their configured USD budgets.
> The conversion logic lives in a shared helper consumed by the sled and in-
> memory marketplaces, while new ledger/explorer regressions replay mixed
> settlements to prove CT/IT totals, USD sums, and oracle snapshots remain aligned
> without depending on a mined block. Explorer, ledger, and dashboard pipelines
> pick up the corrected `SettlementBreakdown` values so USD totals, CT counts, and
> IT counts now stay in lockstep across CI artefacts. Readiness telemetry now
> mirrors the same inputs: snapshots persist both the archived and live oracle
> prices, per-cohort utilisation deltas, and ppm summaries, telemetry exporters
> emit matching gauges, and the metrics aggregator forwards
> `ad_readiness_utilization_{observed,target,delta}_ppm` while pruning stale
> label sets, letting dashboards and CI alert whenever utilisation drifts from
> the governance targets despite steady demand.
> **Review (2025-11-07, afternoon):** Ad settlements now surface USD totals and
> dual-currency token splits. `InMemoryMarketplace::commit` and
> `SledMarketplace::commit` record the posted USD price, oracle snapshot, and
> role-level CT plus IT token quantities inside `SettlementBreakdown`, keeping
> the legacy CT ledger fields accurate while exposing mirrored IT payouts for the
> upcoming dual-token migration. Unit coverage in `crates/ad_market` now exercises
> the USD→CT/IT conversions, token totals, and rounding behaviour, and the RPC
> harness continues to pass with the richer schema. Explorer/Gateway docs will
> follow once the ledger consumes the IT fields.
> **Review (2025-10-27, evening):** Chaos rehearsals now archive preserved manifests
> and mirror bundles without leaving first-party tooling. `sim/chaos_lab.rs`
> generates a run-scoped `manifest.json` under `chaos/archive/<run_id>/` and a
> `latest.json` pointer that records filenames, byte sizes, and BLAKE3 digests for
> every snapshot, diff, overlay readiness table, and provider failover report,
> then writes a deterministic `run_id.zip` bundle. Optional `--publish-dir`,
> `--publish-bucket`, and `--publish-prefix` flags copy the manifests and bundle
> into long-lived directories or S3-compatible buckets via the first-party
> `foundation_object_store` client, which now ships canonical-request tests and a
> blocking upload regression that prove AWS Signature V4 headers match the
> published examples. Upload retries respect the new
> `TB_CHAOS_ARCHIVE_RETRIES` knob (minimum 1) and can pin timestamps with
> `TB_CHAOS_ARCHIVE_FIXED_TIME` so reproducible runs emit the same signed requests
> in integration environments. `tools/xtask` manually decodes the manifests with
> `foundation_serialization::json::Value`, reports publish targets, logs the
> computed BLAKE3 digests and byte sizes for the manifest/bundle pair, and honours
> the new flags alongside existing readiness analytics. Release automation now
> fails immediately when `chaos/archive/latest.json` or the referenced manifest is
> missing, and the verifier parses the manifest to confirm every archived file
> exists, that the recorded bundle size matches the on-disk `run_id.zip`, and that
> object-store mirrors surfaced by `cargo xtask chaos` point at the same files,
> preventing tags when artefacts drift or uploads truncate.
> **Review (2025-10-27, afternoon):** Chaos automation now loops entirely through
> first-party tooling. `sim/chaos_lab.rs` fetches `/chaos/status` baselines with
> `httpd::BlockingClient`, decodes them manually via
> `foundation_serialization::json::Value`, and persists both the status diff and
> overlay readiness rows so long-running soak jobs can diff provider-aware
> regressions without serde stubs or external HTTP clients. The simulator still
> records provider kinds in `SiteReadinessState`, the harness emits mixed-provider
> attestations, and the metrics aggregator mirrors the provider label through
> `/chaos/status`, `chaos_site_readiness{module,scenario,site,provider}`, and the
> persisted diff artefacts while pruning stale labels when sites disappear. The
> new `sim/tests/chaos_harness.rs::reconfiguring_sites_replaces_previous_entries`
> and `metrics-aggregator::tests::chaos_site_updates_remove_stale_entries` guard
> topology churn, and `cargo xtask chaos` now consumes the overlay readiness JSON
> to report module totals, scenario readiness, readiness improvements/regressions,
> provider churn, and duplicate site detection with pure `std` collections. Listener
> binding remains unified: `node/src/net/listener.rs` reports
> `*_listener_bind_failed` warnings across RPC, gateway, status, and explorer
> servers, `explorer/tests/explorer_bind.rs` proves the public HTTP surface logs
> the warning when a port is occupied, and `node/tests/rpc_bind.rs` continues to
> assert sockets surface warnings instead of panics. The ban CLI still ships tests
> for storage/list failures so `ban store error:` warnings bubble through instead
> of silently mutating metrics. Release tooling now shells out to
> `cargo xtask chaos --out-dir releases/<tag>/chaos` inside
> `scripts/release_provenance.sh`, verifies the snapshot/diff/overlay/provider
> failover JSON payloads before hashing binaries, and refuses to proceed when the
> gate fails; `scripts/verify_release.sh` mirrors the guard by erroring whenever a
> published archive omits the `chaos/` directory or any of those files. The
> `just chaos-suite` recipe emits the same artefacts (even without a baseline,
> thanks to explicit empty diff emission) so local drills match the release path.
> **Review (2025-10-26, late night):** Chaos readiness now tracks mixed-provider
> overlays end to end. The simulator seeds overlay scenarios with weighted
> `ChaosSite` entries, the aggregator mirrors them through
> `chaos_site_readiness{module,scenario,site,provider}` and heals poisoned readiness locks
> with a `chaos_status_tracker_poisoned_recovering` warning, and Grafana/JSON
> snapshots remain sorted so automation can diff runs deterministically. The
> mobile sync suite gained a first-party stub for builds without the
> `runtime-wrapper` feature, and a `Node::start` regression binds an occupied
> socket to assert gossip emits `gossip_listener_bind_failed` instead of
> panicking.
> **Review (2025-10-26, late morning):** Liquidity routing now stress-tests
> multi-batch fairness, hop-limited trust fallbacks, and challenged withdrawals.
> `node/tests/liquidity_router.rs` proves excess intents roll into deterministic
> follow-up batches, challenged bridge commitments never reach execution, and
> hop limits downgrade slack-optimised paths to direct fallbacks. Trust-ledger
> coverage (`node/tests/trust_routing.rs`) verifies the slack-aware
> `find_best_path` still surfaces a shortest-path fallback for schedulers with
> tighter hop budgets. Documentation captures the new slack-aware routing model
> and test expectations so operators understand why wider corridors may be
> preferred while fairness remains deterministic.
> **Review (2025-11-06, late morning):** Premium-domain escrow and cancellation
> flows are now fully first-party. Operators deposit CT via `dns.register_stake`,
> withdraw unlocked balances with `dns.withdraw_stake`, and inspect live escrow
> totals (and per-transfer `ledger_events`/`tx_ref` history) through
> `dns.stake_status`. Auction settlement batches every ledger
> debit/credit so failures cannot double-charge bidders or leave sled ahead of
> the CT ledger. Sellers can unwind listings with `dns.cancel_sale`, releasing
> locked bidder/seller stakes without forging sale history. The CLI picked up
> `gateway domain stake-register`, `stake-withdraw`, `stake-status`, and `cancel`
> helpers, while the integration harness now exercises stake deposits, partial
> withdrawals, and cancellation flows alongside the existing ledger-settlement
> tests.
> **Review (2025-11-07, morning):** Gateway `ReadAck`s now include geo, device,
> CRM, delivery-channel, and RangeBoost mesh context derived from new
> `X-TheBlock-*` headers and provider-registered cohort memberships. The ad
> marketplace consumes those selectors alongside badge intent and assigns uplift
> holdouts on every reservation; sled persistence snapshots the estimator so
> restart cycles keep treatment/control counts aligned. Gateway regression suites
> pin the merged CRM list, mesh peer enrichment, and holdout propagation while
> in-memory and sled marketplaces record control impressions without debiting
> spend. `range_boost::best_peer` feeds mesh latency back into the impression
> context, and selection receipts surface the holdout assignment so SDKs can
> suppress control impressions deterministically.
> **Review (2025-11-05, afternoon):** Premium domain auctions finally settle
> entirely on-ledger. `dns.complete_sale` debits the winning bidder, refunds any
> seller stake, books protocol/royalty fees to the treasury or prior owner, and
> records every ledger reference inside sale history so explorers and CLI users
> can audit the transfers. Stake escrows are enforced during bidding, releasing
> automatically when bidders are outbid. Ad readiness counters now persist to a
> dedicated sled namespace; startup replays the surviving window so nodes stay
> “ready” across restarts, and the handle keeps pruning/recording without
> dropping events. Integration coverage under `node/tests/dns_auction_ledger.rs`
> exercises winning/losing settlements against the ledger mock while ad
> readiness tests confirm replayed windows remain satisfied after reopening.
> **Review (2025-11-03, evening):** Ad readiness now gates campaign matching with
> governance-controlled thresholds. The node shares an `AdReadinessHandle` across
> the gateway, RPC stack, and telemetry so readiness blockers and counters surface
> through `ad_market.readiness`, metrics exporters, and dashboards. Gateway tests
> cover readiness skips/unlocks, while the RPC harness proves readiness snapshots
> advance once acknowledgements land. Premium domain auctions now persist entirely
> through first-party cursor codecs: auctions, bids, ownership, and sale history
> live in sled-backed `SimpleDb` buckets, CLI commands manage listing/bidding/
> completion/status, and resale flows enforce stored royalty rates and protocol
> fees. New regressions drive auction expiry, resale royalty distribution, and CLI
> plumbing so operators can stage `.block` sales without external marketplaces.
> **Review (2025-10-25, late evening):** Read acknowledgements now bundle zero-
> knowledge commitments. The gateway stamps each `ReadAck` with a readiness
> snapshot proof and derives a per-ack reservation discriminator, the node
> exposes `--ack-privacy`/`node.set_ack_privacy` toggles, and ledger submission
> logs `read_ack_processed_total{result="invalid_privacy"}` whenever proofs fail
> under observe mode. New regression tests cover the proof round-trip and enforce
> that identical reads no longer collide in the ad marketplace.
> **Review (2025-10-30, early morning):** ANN benchmark automation now writes
> per-run history and regression signals entirely in-house. `TB_BENCH_HISTORY_PATH`
> + `TB_BENCH_HISTORY_LIMIT` persist timestamped percentiles (plus exponentially
> weighted moving averages) alongside `benchmark_*_iterations`, while
> `config/benchmarks/<name>.thresholds` + `TB_BENCH_THRESHOLD_DIR` keep canonical
> regression ceilings versioned and `TB_BENCH_REGRESSION_THRESHOLDS` +
> `TB_BENCH_ALERT_PATH` clamp acceptable latency and emit first-party alert
> summaries. Telemetry ingests the new `benchmark_*_regression` gauges beside the
> existing percentiles so Grafana and the HTML dashboards highlight slow runs
> without third-party tooling. Verifier committee guards now increment
> `ad_verifier_committee_rejection_total{committee,reason}` whenever stake,
> snapshot, or VRF mismatches surface, and the RPC regression suite scrapes the
> metrics exporter after a forced committee failure to lock in the expected
> labels, keeping weight tampering observable in real time. Parser coverage now
> exercises malformed `TB_BENCH_REGRESSION_THRESHOLDS` entries and confirms
> case-insensitive keys so CI can reject slow runs without
> human typos breaking suites, while new committee guard integration tests assert
> the rejection counter increments under missing-snapshot failures. With those
> guardrails in place, `SettlementBreakdown` carries the winning creative’s
> `uplift` payload
> so RPC consumers and explorers correlate pacing analytics with realised lift
> without recomputing estimates off-box.
> **Review (2025-11-10, early morning):** `python_bridge_macros` replaces the
> last proc-macro holdout with first-party `#[new]`/`#[getter]`/`#[setter]`/
> `#[staticmethod]` stubs so the node’s Python bindings build without `pyo3`, and
> the bindings keep re-exporting them through the in-tree bridge. `serve_metrics`
> now installs the `foundation_metrics` recorder on demand and exposes helpers to
> reset/register `ad_verifier_committee_rejection_total{committee,reason}`
> handles before RPC tests scrape the exporter, eliminating cross-test leakage.
> Benchmark automation warns on unsupported regression keys (`p42`, etc.) while
> skipping them, and history CSVs preserve EWMA columns even when a run omits
> percentile samples, so dashboards keep a continuous latency trend without
> suggesting zero-cost iterations.
> **Review (2025-11-06, afternoon):** Advertising settlements now show the full
> dual-token story across the explorer, CLI, telemetry, and readiness RPC. The
> ledger codecs persist CT totals, IT totals, USD micros, settlement counts, and
> oracle snapshots so downstream tooling never has to infer conversion math, and
> the explorer/CLI views render the new fields with regression coverage for
> binary and JSON paths. The metrics aggregator seeds a dedicated
> `explorer_block_payout_ad_it_total{role}` counter family and publishes peer-
> scoped gauges for `explorer_block_payout_ad_usd_total`,
> `explorer_block_payout_ad_settlement_count`, and the CT/IT oracle prices each
> block consumed, while `ad_market.readiness` now embeds both the persisted
> snapshot and the live marketplace oracle under a single `oracle` object plus a
> `utilization` summary. Dashboards and CI artefacts pick up the same gauges so
> readiness automation, explorer views, and release checks stay aligned on the
> CT/IT split and the utilisation driving price updates.
> **Review (2025-10-25, evening):** Governance parameter activations now update
> the live ad revenue split in lockstep with policy votes. The node runtime wires
> the shared marketplace handle into the read-subsidy apply hooks, and the new
> `governance_updates_distribution_policy` integration test toggles the five
> `read_subsidy_*_percent` knobs at runtime before round-tripping the updated
> `ad_market.distribution` response through the RPC harness. Budget reservations
> are tracked atomically across both in-memory and sled backends via a
> `pending` ledger that commits on success and refunds on cancel, with a
> multi-threaded regression proving only funded impressions proceed while the
> campaign balance drains deterministically. Together the pipeline keeps ad
> payouts aligned with governance knobs and seals the oversubscription hole that
> previously left unpaid acknowledgements.
> **Review (2025-10-25, afternoon):** Workspace clippy now runs clean across the
> foundation serialization layer (`foundation_serde`, `foundation_serialization`),
> crypto primitives, concurrency helpers, and the bespoke HTTP/runtime shims by
> replacing placeholder iterators, lifetimes, and inherent `to_string` methods
> with first-party Display impls and lock guards. The `integration-tests`
> harness for `ad_market` exercises distribution updates and a concurrent
> registration race so only one campaign lands while the duplicate path
> preserves its error code, and the Justfile’s `test-gateway` recipe scopes the
> web suite directly to `web::gateway::tests::` so CI keeps the feature-gated
> server green.
> **Review (2025-10-28, evening+):** Selection receipts now bundle a
> BLAKE3-committed candidate trace, and the marketplace validates wallet-supplied
> SNARK proofs through the new `zkp::selection` module while tolerating TEE
> fallbacks only when governance allows them. Missing attestations now respect
> `require_attestation` without aborting reservations, and telemetry records the
> accepted/rejected mix per attestation kind. The cohort PI controller exports its
> error, integral term, forgetting factor, and saturation counters so Grafana can
> chart price damping, while the `ad_budget_progress` gauge reports how much of a
> campaign’s USD allocation is reserved. Badge guard scaffolding now logs
> per-cohort populations and auto-relaxes predicates when active populations fall
> below `k_min`, keeping targeting honest without removing the first-party guard.
> **Review (2025-10-25, mid-morning):** Gateway provider inference now resolves
> operators from storage manifests (falling back to deterministic hashing when
> multiple providers are eligible) and the test harness exposes override hooks
> so unit suites stay hermetic. The new `node/tests/ad_market_rpc.rs` drives the
> RPC router in-process with a sled-backed marketplace, covering successful
> registrations plus duplicate and malformed payload errors under the
> `integration-tests` feature without opening sockets. Monitoring’s payout row
> wires the last-seen gauges into the Grafana snapshots and the shared
> `alert_validator` dataset so the `Explorer*PayoutStalled` rules stay audited,
> and the `Justfile` now ships a `test-gateway` recipe that runs the
> feature-gated gateway suite against the normalized module layout.
> **Review (2025-10-24, late night):** The ad marketplace graduated from an
> in-memory stub to a sled-backed store. `SledMarketplace` now persists
> campaigns, budgets, and distribution policies so governance can audit live
> spend after restarts, and the node exposes first-party RPC handlers for
> `ad_market.inventory`, `ad_market.distribution`, and
> `ad_market.register_campaign`. The contract CLI wires a dedicated
> `ad-market` subcommand, while the explorer’s `block-payouts` command picks up
> table/Prometheus output modes for automation. Gateway matching consumes the
> refreshed service-badge registry—providers register physical-presence badges
> that the web pipeline now threads through impression context—with an
> end-to-end regression guarding badge-targeted campaigns. Metrics gain
> `explorer_block_payout_{read,ad}_last_seen_timestamp{role}` gauges and paired
> staleness alerts so dashboards and paging catch silent payout pipelines
> without leaving the first-party stack.
> **Review (2025-11-02, evening):** Bridge CLI parser coverage now walks the
> settlement-log and reward-accrual commands through the in-house `Parser`,
> locking optional asset/relayer filters, cursor forwarding, and the default
> 50-row page size without reintroducing serde helpers. Node-side coverage adds
> `bridge_pending_dispute_persists_across_restart`, which spins a challenged
> withdrawal, restarts the sled-backed bridge store, and proves both the
> `pending_withdrawals` surface and `bridge.dispute_audit` RPC retain the
> challenger metadata. Monitoring’s dashboard harness gained
> `dashboards_include_bridge_remediation_legends_and_tooltips`, asserting every
> remediation panel keeps its first-party legends and descriptions so Grafana
> tooltips stay aligned with the PromQL across all generated templates.
> **Review (2025-10-31, late evening):** Bridge remediation’s `RemediationSpoolSandbox` now enables page/throttle/quarantine/escalate targets, proves environment guards unwind via `remediation_spool_sandbox_restores_environment`, and keeps every retry scenario hermetic without `/tmp` residue. Explorer payout coverage stacks `explorer_payout_counters_are_peer_scoped` on top of the churn regression, demonstrating per-peer caches remain monotonic even as explorers report disjoint totals—all within the first-party HTTPd harness.
> **Review (2025-10-31, afternoon):** Bridge remediation regression suites gained a `RemediationSpoolSandbox` helper that seeds per-test directories, wires `TB_REMEDIATION_*_DIRS` env guards, and tears everything down automatically so retry-heavy cases stop polluting `/tmp`. Explorer payout coverage now alternates read and advertising role sets through `explorer_payout_counters_remain_monotonic_with_role_churn`, proving the cache ignores regressions and only emits deltas for new highs while the aggregator keeps trace-only diagnostics for decreases.
> **Review (2025-10-26, evening):** `node/tests/read_ack_privacy.rs` now shares a
> first-party `concurrency::Lazy` fixture so readiness snapshots and signatures
> only build once per run, shrinking test runtime while keeping tamper coverage.
> The ad marketplace holds the pending-budget lock through reservation
> insertion—both in-memory and sled implementations now refuse to oversubscribe
> campaigns even when `reserve_impression` races—backed by a new concurrent
> reservation regression. Grafana’s compute dashboard adds a “Read Ack Outcomes”
> panel charting `read_ack_processed_total{result}` so the
> `result="invalid_privacy"` series rides alongside the existing
> `ok`/`invalid_signature` counters.
> **Review (2025-10-27, evening):** Ad pricing now runs a log-domain PI controller
> with exponential forgetting (`|η_P|≤0.25`, `η_I≤0.05|η_P|`) so thin cohorts stop
> oscillating while converging toward the governance utilisation target.
> `SelectionReceipt::validate` guards composite resource floors, quality-adjusted
> second-price clears, and attestation structure before the node accepts an ack,
> and telemetry adds `read_selection_proof_{verified,invalid}_total{attestation}`
> plus `read_selection_proof_latency_seconds{attestation}` so SNARK/TEE mixes and
> proof lag appear in dashboards.
> **Review (2025-10-29, morning):** Selection receipts now recompute the SNARK
> commitment and transcript digest via `zkp::selection`, rejecting receipts whose
> metadata or witness digests diverge from the canonical circuit inputs. The ad
> market budget RPC fans snapshots into the telemetry module, emitting
> `ad_budget_config_value{parameter}`, `ad_budget_campaign_{remaining_usd,dual_price,epoch_target_usd}`,
> `ad_budget_shadow_price{campaign}`, `ad_budget_kappa_gradient{campaign,...}`,
> cohort-level `ad_budget_cohort_{kappa,error,realized_usd}` gauges, and the
> `ad_resource_floor_component_usd{component}` breakdown with first-party label
> lifetimes so dashboards expose pacing pressure and composite floors without
> third-party metrics crates.
> **Review (2025-10-24, early afternoon):** Explorer integration now mines blocks that mix binary headers with JSON fallbacks so payout decoding stays resilient across codec boundaries, and the metrics aggregator records the role-labelled counters directly via cached `CounterVec` handles. The `/metrics` integration asserts both `explorer_block_payout_read_total` and `_ad_total` advance on a second scrape, mirroring the Grafana PromQL so the dashboards stay backed by live data. Documentation now includes CLI automation snippets for hash and height payout queries, plus monitoring notes covering the new counter caching path so operators know where the deltas originate.
> **Review (2025-10-30, morning):** Explorer payout queries now guard the JSON fallback so legacy snapshots lacking `read_sub_*` or `ad_*` fields still render per-role totals, and new unit tests pin that behaviour to FIRST_PARTY_ONLY runs. The CLI suite exercises the failure paths for unknown hashes/heights and the mutual-exclusion flag checks, while the Grafana generator adds a “Block Payouts” row that charts read-subsidy and advertising role counters. Operators can now move from database snapshots, through automation, to dashboards without leaving first-party surfaces.
> **Review (2025-10-29, early morning):** Read acknowledgements now propagate through
> the node’s background worker, which increments `read_ack_processed_total{result}`
> and populates per-role epoch ledgers. `Blockchain::finalize_block` uses the new
> governance split (`read_subsidy_*_percent`) to mint viewer/host/hardware/verifier/
> liquidity payouts, persists the totals in `read_sub_*_ct`, and flushes matched ad
> campaign settlements into `ad_*_ct`. The `ad_market` crate powers campaign
> matching, gateway batches hash the expanded metadata, and the mobile cache tests
> exercise the first-party binary cursor codec so persistence no longer trips the
> `foundation_serde` stub. Explorers, dashboards, and docs should surface the
> per-role subsidy and advertising fields so operators can reconcile attention
> payouts without replaying raw receipts.
> **Review (2025-10-28, evening):** Gateway read acknowledgements now require
> callers to supply signed payload headers. `web/gateway.rs` derives
> `client_hash = blake3(domain || client_ip)`, verifies the Ed25519 signature
> attached via the `X-TheBlock-Ack-*` headers, and drops zeroed acks unless the
> legacy compatibility feature is enabled. The gateway test suite injects
> synthetic signatures through the in-memory transport to prove the queue only
> receives verifiable receipts, and the read-receipt documentation spells out
> the signing contract for wallet and SDK integrations.
> **Review (2025-10-27, late afternoon):** Bridge remediation spool directories
> now retain retry artefacts across acknowledger restarts and flush them once
> hooks acknowledge or close the action. The restart suite exercises the
> cleanup path, the contract remediation CLI surfaces per-action
> `spool_artifacts` in both filtered and JSON outputs, and the monitoring test
> harness now verifies both the latency policy overlays and the new
> `bridge_remediation_spool_artifacts` gauge/panel that tracks outstanding spool
> payloads.
> **Review (2025-10-27, morning):** Bridge dashboards now overlay the policy
> gauge `bridge_remediation_ack_target_seconds{playbook,policy}` on the
> acknowledgement latency histogram, the metrics aggregator restores the
> histogram samples after a restart, and a new `BridgeRemediationAckLatencyHigh`
> alert fires when p95 breaches the configured target. The `contract
> remediation bridge` CLI gained `--playbook`, `--peer`, and `--json` flags so
> on-call responders and automation can filter or stream persisted actions
> without leaving the first-party stack.
> **Review (2025-10-25, mid-morning):** Governance escalations now surface
> acknowledgement telemetry end to end. The metrics aggregator records
> `bridge_remediation_dispatch_ack_total{action,playbook,target,state}` whenever
> paging/governance hooks respond, persists acknowledgement timestamps and notes
> on each remediation action, and exposes the fields via
> `/remediation/bridge`/`/remediation/bridge/dispatches`. A new Grafana panel
> charts five-minute acknowledgement deltas alongside the existing dispatch and
> action panels so operators can prove downstream workflows closed the loop.
> CLI/aggregator integration tests swap in a first-party HTTP override client to
> script acknowledgement payloads without a server, locking the new counters and
> fields to FIRST_PARTY_ONLY coverage.
> **Review (2025-10-24, pre-dawn):** Bridge anomaly handling now drives automated
> remediation backed by persistent state. The metrics aggregator records per-
> relayer actions, exposes `/remediation/bridge`, increments
> `bridge_remediation_action_total{action,playbook}`, and resumes recommendations
> after restarts via a dedicated sled column family while tagging each action
> with the follow-up playbook (`incentive-throttle` or `governance-escalation`).
> Cross-chain liquidity counters
> (`bridge_liquidity_locked_total{asset}`,
> `bridge_liquidity_unlocked_total{asset}`,
> `bridge_liquidity_minted_total{asset}`, and
> `bridge_liquidity_burned_total{asset}`) now feed the same anomaly baselines and
> dashboards so reward anomalies can be correlated with asset-specific flows.
> Alert validation graduated to the new `monitoring/src/alert_validator.rs`
> module; the existing
> `bridge-alert-validator` binary invokes the shared helper to replay canned
> datasets for the bridge, chain-health, dependency-registry, and treasury alert
> groups so CI keeps every Prometheus expression hermetic without promtool.
> **Review (2025-10-23, late evening):** Bridge alerting now pairs aggregate and
> per-label skew detection with first-party validation. Prometheus rules
> `BridgeCounterDeltaLabelSkew`/`BridgeCounterRateLabelSkew` monitor
> `labels!=""` selectors alongside the existing aggregate gauges, and the
> validator binary parses `monitoring/alert.rules.yml`, normalises the
> expressions, and replays canned datasets. CI runs the validator with
> `cargo test --manifest-path monitoring/Cargo.toml`, keeping alert coverage
> hermetic without promtool while the Grafana/HTML dashboards already chart the
> per-label gauges for incident response.
> **Review (2025-10-23, evening):** External settlement proofs now require the
> deterministic `settlement_proof_digest` and track per-asset height watermarks,
> rejecting mismatched hashes and replayed heights before rewarding a duty.
> Successful deposits, settlement submissions, and withdrawal finalisation all
> append `RewardAccrualRecord` entries that the new `bridge.reward_accruals`
> RPC/CLI paginates for operators. CLI integration coverage adds a
> `BridgeCmd::RewardAccruals` regression, node-side tests exercise hash/height
> failure paths plus accrual pagination, and telemetry snapshots cover the new
> counters so dashboards chart reward intake end to end.
> **Review (2025-10-22, evening+):** Multi-asset bridge supply tracking now spans
> the token bridge, node RPC, and CLI. `TokenBridge` records locked and minted
> balances per symbol, persistence in `bridges/src/codec.rs` includes the new
> fields, the node exposes `Bridge::asset_snapshots()` and the
> `bridge.assets` RPC returns structured entries (symbol, emission, locked,
> minted) that the CLI and integration tests assert end to end. The metrics
> aggregator gained a bridge anomaly detector that watches the reward, approval,
> settlement, and dispute counters, increments `bridge_anomaly_total`, and
> serves `/anomalies/bridge` so dashboards chart five-minute increases alongside
> the raw counters. Fresh tests cover anomaly spikes, cooldown enforcement, and
> CLI asset pagination under FIRST_PARTY_ONLY.
> **Review (2025-10-22, late):** The anomaly detector now exports
> `bridge_metric_delta{metric,peer,labels}` and
> `bridge_metric_rate_per_second{metric,peer,labels}` gauges through the
> Prometheus-compatible recorder so dashboards and alerting rules can graph
> per-relayer growth without scraping raw JSON. The CLI/HTTP integration suite
> asserts both unlabeled and asset-scoped gauge lines while dropping the
> temporary debugging prints used during bring-up.
> **Review (2025-10-22, afternoon):** The bridge CLI integration suite now drives
> `BridgeCmd::DisputeAudit` through the in-memory `MockTransport`, capturing the
> JSON-RPC envelopes and paginated responses alongside the existing claim and
> settlement coverage so FIRST_PARTY_ONLY builds exercise every audit surface. The
> monitoring templates gained a dedicated Bridge row that charts five-minute
> `increase()` deltas for `bridge_reward_claims_total`,
> `bridge_reward_approvals_consumed_total`,
> `bridge_settlement_results_total{result,reason}`, and
> `bridge_dispute_outcomes_total{kind,outcome}`, with the dashboard snapshot test
> updated to expect the new panels. Grafana imports and the HTML snapshot now
> expose bridge reward consumption, settlement outcomes, and dispute resolutions
> without third-party widgets.
> **Review (2025-10-23, afternoon):** Contract CLI bridge commands now route all
> JSON-RPC traffic through a new `BridgeRpcTransport` trait. Production still
> wraps the in-house `RpcClient`, but the integration suite swaps in an
> in-memory `MockTransport` that records envelopes and returns scripted
> responses. The test harness sheds the HTTP `JsonRpcMock` server and async
> runtime dependency, keeping FIRST_PARTY_ONLY runs hermetic while continuing to
> exercise the CLI pagination flows (`reward-claims`, `settlement-log`, and
> `dispute-audit`) and settlement/reward payload builders end to end.
> **Review (2025-11-01, early morning):** Bridge CLI coverage now verifies the optional-path contract end to end. `bridge_dispute_audit_serializes_optional_fields` drives `BridgeCmd::DisputeAudit` with `asset=None`/`cursor=None`, asserting the JSON envelope emits explicit `null` fields while still applying the default page size. `bridge_dispute_audit_parser_defaults_limit_and_cursor` runs the command builder through the in-house parser so FIRST_PARTY_ONLY runs prove the 50-entry default and localhost fallback survive future refactors. Monitoring picked up `dashboards_include_bridge_counter_panels`, which parses every generated Grafana JSON (`dashboard`, `operator`, `telemetry`, `dev`) to ensure the reward-claim, approval, settlement, and dispute panels keep their intended queries and legends in lockstep across templates. These guards keep governance audit tooling hermetic without reintroducing third-party transports or dashboard validators.

> **Review (2025-10-23, pre-dawn):** Bridge incentives now cover the full governance lifecycle. Reward claim approvals persist through the sled-backed governance store, `bridge.claim_rewards` consumes allowances, and the CLI surfaces `blockctl bridge claim`/`reward-claims` so operators can reconcile payouts without touching sled snapshots. Cursor/limit pagination plus `next_cursor` responses landed across `bridge.reward_claims`, `bridge.settlement_log`, and `bridge.dispute_audit`, keeping audits streaming without dumping entire deques. Settlement proofs gained first-party RPC/CLI (`bridge.submit_settlement`, `bridge.settlement-log`), `bridge.dispute_audit` renders challenge/settlement summaries, and channel configuration allows partial updates with optional proof enforcement. Telemetry exports now include `BRIDGE_REWARD_CLAIMS_TOTAL`, `BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL`, `BRIDGE_SETTLEMENT_RESULTS_TOTAL{result,reason}`, and `BRIDGE_DISPUTE_OUTCOMES_TOTAL{kind,outcome}`, with the refreshed `node/tests/bridge_incentives.rs` suite verifying counter increments under `test-telemetry`. Fresh tests in `governance/src/store.rs` and `node/src/governance/store.rs` lock the new flows to FIRST_PARTY_ONLY builds.
> **Review (2025-10-22, mid-morning+):** CLI integration tests now exercise the
> wallet preview helpers directly: the `fee_floor_warning` suite asserts the
> `signer_metadata` vector for ready and override branches, and a dedicated
> `wallet_signer_metadata` test module snapshots the metadata array for local,
> ephemeral, and session signers while inspecting the emitted telemetry event
> on auto-bump flows. The assertions operate on first-party `JsonMap` builders
> so the JSON surface stays deterministic without relying on serde helpers or
> mock RPC servers. `cargo test --manifest-path cli/Cargo.toml` passes with the
> extended coverage, keeping FIRST_PARTY_ONLY runs hermetic.
> **Review (2025-10-22, early morning):** Wallet preview coverage now asserts
> on the signer metadata emitted in ready paths, locking the new
> `BuildTxReport::signer_metadata` field to deterministic JSON across auto-bump,
> confirmation, ephemeral, and session flows while snapshotting the JSON array
> so future refactors keep the output stable. Service-badge and telemetry commands gained
> helper-backed unit tests that snapshot the JSON-RPC envelopes produced by
> `verify`, `issue`, `revoke`, and `telemetry.configure`, keeping the CLI suite
> first-party without mock servers or serde conversions. The remaining example
> binaries shed their last `foundation_serialization::json!` literals—mobile
> push notifications and the node difficulty probe now assemble payloads through
> manual `JsonMap` builders—so documentation tooling mirrors the production
> JSON façade. `cargo test --manifest-path cli/Cargo.toml` passes with the new
> metadata assertions, matching expectations for FIRST_PARTY_ONLY runs.
> **Review (2025-10-21, evening++):** Treasury CLI coverage now exercises the
> new helper surfaces directly: lifecycle tests fund the sled-backed store via
> `GovStore::record_treasury_accrual`, execution/cancel flows assert on the
> typed status records, and remote fetch tests validate `combine_treasury_fetch_results`
> for both populated and empty history responses without calling
> `foundation_serialization::json::to_value`. The refactor eliminates the last
> `JsonRpcMock`/serde stub dependency in the CLI suite, keeping FIRST_PARTY_ONLY
> runs hermetic while documenting the helpers’ behaviour across optional
> cursors. A full `cargo test --manifest-path cli/Cargo.toml --tests` run now
> completes without timeouts, and `cargo test --manifest-path node/Cargo.toml --lib`
> was rerun to green to lock in the node-side regression coverage after the CLI
> manualization.
> **Review (2025-10-21, mid-morning):** CLI JSON builders are now centralized
> behind a dedicated `json_helpers` module that exposes string/number/null
> constructors plus JSON-RPC envelope helpers. The compute, service-badge,
> scheduler, telemetry, identity, config, bridge, and TLS commands have been
> rewritten to compose request payloads through those builders, eliminating every
> remaining `foundation_serialization::json!` literal across the contract CLI
> surface. Node-side consumers follow suit: the runtime log sink builds its map
> manually, the wallet binary emits staking and escrow payloads via the same
> helpers, and governance list output serializes through a tiny typed wrapper
> instead of an ad-hoc macro. With the shared helpers in place, JSON-RPC traffic
> from both the CLI and wallet binaries stays entirely inside first-party
> `JsonMap` assembly while preserving legacy response shapes and deterministic
> ordering.
> **Review (2025-10-21, pre-dawn):** Governance webhook delivery now runs in
> every build: `telemetry::governance_webhook` always posts when
> `GOV_WEBHOOK_URL` is set instead of silently short-circuiting when the
> `telemetry` feature is disabled. The CLI’s networking surfaces shed the
> `foundation_serialization::json!` macro in favour of a shared `RpcRequest`
> helper plus explicit `JsonMap` builders, covering `net`, `gateway`,
> `light_client`, and `wallet` commands so JSON-RPC envelopes and error payloads
> assemble deterministically. The node’s `net` binary mirrors the same manual
> builders, keeping request batching, export helpers, and throttle operations on
> the first-party JSON façade. Follow-up unit coverage continues to pass under
> FIRST_PARTY_ONLY, and the CLI tests compile without macro literals on the hot
> networking paths.
> **Review (2025-10-20, near midnight):** Transaction admission now derives a
> priority tip automatically when callers omit it. `Blockchain::submit_transaction`
> subtracts the current base fee from `payload.fee` before computing
> fee-per-byte, so legacy builders that only populate `payload.fee` no longer
> trip `TxAdmissionError::FeeTooLow` under the lane minimum. The base-fee
> regression (`tests/base_fee::adjusts_base_fee_and_rejects_underpriced`) runs
> cleanly with FIRST_PARTY_ONLY enforced. Governance retuning dropped the last
> `foundation_serde` derive: Kalman state snapshots decode via
> `foundation_serialization::json::Value` and re-emit through a first-party map
> builder, unblocking the inflation retune path under the stub backend.
> **Review (2025-10-20, late evening):** Canonical transaction helpers now
> bypass the `foundation_serde` stub entirely. `canonical_payload_bytes` routes
> through `transaction::binary::encode_raw_payload`, `verify_signed_tx` hashes
> signed transactions with the manual writer, the Python bindings decode via
> `decode_raw_payload`, and the CLI reuses the node helper when signing. With
> every RawTxPayload path on the cursor facade, the base-fee regression no
> longer trips the stub serializer and FIRST_PARTY_ONLY builds stay inside
> first-party codecs end to end.
> **Review (2025-10-20, afternoon++):** Peer metric helpers now sort drop and
> handshake reason maps before emitting JSON, and new unit tests lock the
> deterministic ordering to stop flakey assertions as we continue the RPC JSON
> refactor. Compute-market responders (`scheduler_stats`, `job_requirements`,
> `provider_hardware`, and the settlement audit log) assemble payloads through
> first-party builders that reuse the shared map helper, ensuring capability,
> utilization, and audit records render without `json::to_value` fallbacks while
> keeping optional fields aligned with legacy responses. DEX escrow status and
> release handlers now encode payment proofs, Merkle paths, and roots via
> in-house `Value` construction, dropping the last serde-based conversions on
> that surface and preserving the legacy array layout for proofs and roots.
> **Review (2025-10-20, midday):** Manual cursor writers for blocks, transactions,
> and gossip payloads now delegate field emission to `StructWriter::write_struct`,
> eliminating the hand-maintained field counters that previously triggered
> `Cursor(UnexpectedEof)` when layouts drifted. The binary cursor exposes
> `field_u8`/`field_u32` helpers so codecs describe their schema inline without
> closure boilerplate, and fresh round-trip tests cover block, blob transaction,
> and gossip message encoders under the in-house cursor. RPC peer statistics
> handlers dropped the last `json::to_value` usage in favour of deterministic map
> builders, keeping aggregator exports and `net.peer_stats_export_all` wired
> through first-party JSON assembly end to end.
> **Review (2025-10-20, morning):** Ledger snapshots now persist cached
> transaction sizes and decode helpers cover every legacy cursor entry. The
> mempool writer stores `serialized_size` for each `MempoolEntryDisk`, the
> startup rebuild consumes that cached byte length before re-encoding, and new
> `ledger_binary` tests exercise `decode_block_vec`, `decode_account_map_bytes`,
> `decode_emission_tuple`, and the legacy mempool entry layout so FIRST_PARTY_ONLY
> runs catch regressions without falling back to `binary_codec`.
> **Review (2025-10-19, afternoon):** Storage provider-profile regression tests
> no longer depend on `binary_codec`—the suite now generates its "legacy"
> fixtures with the same cursor writer as production code, while randomized
> EWMA/throughput coverage continues to run under `FIRST_PARTY_ONLY`. Gossip
> peer telemetry and aggregator failover tests switched to the shared
> `peer_snapshot_to_value` helper so unit tests assert against the in-house JSON
> builders instead of serde-derived payloads, keeping the networking pipeline on
> first-party serialization end to end.
> **Review (2025-10-19, midday):** The node RPC client now builds JSON-RPC
> envelopes manually using `foundation_serialization::json::Value` and parses
> responses without relying on `foundation_serde` derives. QoS/mempool/stake
> calls reuse shared map builders, invalid envelopes surface through
> `RpcClientError::InvalidResponse`, and FIRST_PARTY_ONLY builds no longer trip
> the stub backend when issuing or decoding client requests.
> **Review (2025-10-19, early morning):** Gossip, ledger, and transaction
> payloads now encode exclusively through the first-party binary cursor. The
> networking layer introduces `net::message::encode_message`/`encode_payload`
> helpers that sign and transport `Message`/`Payload` variants without the
> deprecated `foundation_serialization::json!` macro or the legacy
> `binary_codec` shim; a new test suite exercises every payload branch
> (handshake, peer lists, transactions, blob chunks, block/chain broadcasts, and
> reputation updates) plus full message round-trips with optional partition and
> QUIC fingerprint headers. Ledger persistence gained dedicated
> `transaction::binary` and `block_binary` modules that encode raw payloads,
> signed transactions, blob transactions, and full blocks via the shared cursor
> utilities with parity fixtures for quantum and non-quantum builds. Networking
> regression coverage now sorts drop and handshake maps before asserting on the
> encoded layout so deterministic ordering mirrors the manual writers, and the
> DEX/storage manifest tests inspect cursor output directly instead of
> round-tripping through `binary_codec`, eliminating the remaining serde-backed
> sled snapshots.
> **Review (2025-10-18, late night+++):** Jurisdiction policy packs gained typed
> diff helpers, manual binary codecs, and dual-format persistence. `PolicyDiff`
> now records consent/feature deltas as structured `Change<T>` records that round-
> trip through the JSON facade, the new `codec` module encodes packs, signed
> entries, and diffs via the cursor helpers with dedicated regression tests, and
> `persist_signed_pack` writes both JSON and `.bin` snapshots so sled-backed
> stores and legacy tooling stay in sync. Workspace tests exercise the diff API
> end to end (`tests/jurisdiction_dynamic.rs`) while `cargo test -p jurisdiction`
> locks codec coverage on the stub backend.
> **Review (2025-10-18, late night):** Treasury RPC coverage now spans the HTTP
> server and CLI. `gov.treasury.disbursements`, `gov.treasury.balance`, and
> `gov.treasury.balance_history` ship typed request/response structs, the new
> integration test (`node/tests/rpc_treasury.rs`) exercises the dispatcher end
> to end, and `contract gov treasury fetch` folds the responses into a single
> document with user-friendly transport error reporting. The metrics aggregator
> accepts legacy balance snapshots that encoded numbers as strings, warns when
> disbursement state exists without matching balance history, and continues to
> prefer sled data when `AGGREGATOR_TREASURY_DB` is set.
> **Review (2025-10-18, evening):** Treasury persistence now writes sled-backed
> balance/disbursement trees alongside the JSON snapshots, and the node mirrors
> the same helpers so explorer/CLI callers see consistent history. Miner rewards
> honour the new `treasury.percent_ct` parameter—coinbase amounts divert the
> configured share into the governance store before updating emission totals.
> `governance::Params` exposes `to_value`/`deserialize`, bridge RPC handlers use
> typed request/response structs with shared commitment decoding, and new tests
> cover treasury accrual/execute/cancel flows plus mining-driven balance growth.
> **Review (2025-10-16, evening++)**: The serialization facade now passes its
> full stub-backed test suite. The `foundation_serialization::json!` macro gained
> nested-object and identifier-key support with regression tests, every binary,
> JSON, and TOML fixture ships handwritten `Serialize`/`Deserialize`
> implementations, and the `foundation_serde` stub now exposes direct `visit_u8`
> / `visit_u16` / `visit_u32` hooks so tuple decoding no longer panics under the
> guard. FIRST_PARTY_ONLY runs can rely on the facade without skipping fixtures
> or falling back to legacy derives.
> **Review (2025-10-16, late afternoon):** RPC envelopes now stay entirely inside
> the first-party stack. `foundation_rpc::Request` picked up `with_id`/`with_badge`
> builders and `Response::into_payload` decodes typed payloads through the new
> `ResponsePayload<T>` helper, letting `node/src/rpc/client.rs` drop bespoke JSON
> structs and depend solely on the facade for success/error branching. Paired
> updates add `Display` to `foundation_serialization::json::Value` plus a compact
> renderer regression test so RPC callers regain `.to_string()` ergonomics
> without reintroducing serde_json.
> **Review (2025-10-16, midday):** QUIC peer-cert persistence now rewrites `quic_peer_certs.json` in a peer-sorted,
> provider-sorted layout so guard fixtures and operator diffs remain stable even when the in-memory cache shuffles.
> Fresh unit tests cover the disk-entry helper—verifying peer/provider ordering, history vectors, and rotation counters—while
> snapshot helpers reuse the same sorted view so CLI/RPC surfaces continue to emit deterministic payloads.
> **Review (2025-10-16, dawn+):** Light-client persistence now carries first-party ordering guarantees across chunks, snapshots,
> and disk caches. Manual serializers sort account entries before emitting bytes so the new `SNAPSHOT_FIXTURE` and refreshed
> `PERSISTED_STATE_FIXTURE` stay stable with `FIRST_PARTY_ONLY` forced to `1`, `0`, or unset. Unit and integration tests permute
> account orderings, run guard-on/off encode/decode cycles, and exercise the compressed snapshot path via the in-house
> `coding::compressor_for("lz77-rle", 4)` facade, ensuring both parity and rollback coverage.
> **Review (2025-10-14, closing push+++):** RPC fuzz harnesses now build identity
> state inside per-run `sys::tempfile` directories and exercise
> `run`/`run_with_response`/`run_request` directly, removing the shared
> `fuzz_handles`/`fuzz_dids` paths that previously leaked sled state across runs.
> The sled legacy importer’s builder (`legacy::Config`) now drives the node’s
> migration path and ships new round-trip tests that populate multiple trees,
> flush manifests, and reopen them through the first-party reader. The legacy
> manifest CLI gained deterministic ordering and default-column coverage with
> fresh integration tests that hammer multi-CF exports, proving the manifest
> shim stays purely in-house while tooling migrations continue.
> **Review (2025-10-14, near midnight++):** Jurisdiction policy packs now round-
> trip via handwritten JSON helpers instead of serde derives. `PolicyPack` and
> `SignedPack` expose `from_json_value`, `from_json_slice`, and `to_json_value`
> so RPC, CLI, and governance tooling can manipulate raw
> `foundation_serialization::json::Value` data without third-party codecs. The
> crate logs law-enforcement appends through `diagnostics::log`, new tests cover
> array/base64 signature decoding plus malformed pack rejection, and the
> dependency inventory drops the final `log` reference.
> **Review (2025-10-14, late evening+++):** Dependency governance automation now
> ships machine-readable summaries and dashboard coverage. The registry runner
> writes `dependency-check.summary.json` alongside telemetry/violations,
> `tools/xtask` prints the parsed verdict during CI preflights, and
> `scripts/release_provenance.sh` hashes the summary plus telemetry artefacts
> before signing provenance so releases publish drift context with the binary
> SBOM. Monitoring picked up dedicated dependency panels/alerts: new metrics
> definitions render policy status, drift counts, and registry freshness in the
> Grafana JSON, and alert rules page when drift reappears or snapshots go stale.
> CLI/release integration tests enforce the summary contract while regenerated
> dashboards and snapshots keep monitoring builds green.
> **Review (2025-10-14, midday++):** Dependency registry check mode emits
> actionable telemetry and drift narratives. The new `check` module compares
> baseline and generated registries, enumerating additions, removals,
> field-level changes, root-package churn, and policy diffs before persisting a
> `dependency-check.telemetry` snapshot. CLI coverage exercises the failure path
> end-to-end, asserting the drift message, `dependency_registry_check_status`
> label, and per-kind gauges while verifying snapshot/violation artefacts stay
> intact after an error. Metadata coverage added a platform-target fixture that
> validates optional dependencies, cfg-gated edges, and default-member fallbacks
> so `compute_depths` remains correct across large and platform-specific graphs.
> **Review (2025-10-14, pre-dawn++):** Tooling automation now owns the
> dependency registry end-to-end. The CLI exposes a reusable runner that writes
> registry JSON, snapshots, manifest lists, telemetry, and violation reports in
> one pass, returns `RunArtifacts` for downstream automation, and honours a
> `TB_DEPENDENCY_REGISTRY_DOC_PATH` override so integration tests can exercise
> the full flow without mutating committed docs. A new CLI integration test
> drives that runner against the fixture workspace, asserting JSON payloads,
> telemetry counters, snapshot emission, and manifest contents. Registry parser
> coverage now includes a synthetic metadata graph with optional, git, and
> duplicate edges to lock in adjacency deduplication, reverse-dependency
> tracking, and origin detection across less-common workspace layouts, while log
> rotation writes gained a rollback guard to restore the prior ciphertext if any
> sled write fails mid-rotation.
> **Review (2025-10-14, late night+):** Dependency-policy tooling is now fully
> first party. `foundation_serialization::toml` exposes low-level
> `parse_table`/`parse_value` helpers so the dependency registry parses policy
> files without serde derives, the config layer normalises tiers/settings by
> hand, and JSON snapshots round-trip through handwritten
> `foundation_serialization::json::Value` conversions. Unit/integration tests
> run under the stub backend without skips, and a new TOML fixture exercises the
> raw parser to guard regressions while the CLI emits snapshots/violations via
> `json::to_vec_value`.
> **Review (2025-10-14, late night):** Log index key rotation now stages every
> decrypted payload before writing so failures never leave the sled store in a
> mixed-key state. The test suite gained explicit coverage for the failure path
> (`rotate_key_is_atomic_on_failure`) and the JSON backend probe now attempts a
> full `LogEntry` round-trip so FIRST_PARTY_ONLY runs skip gracefully when the
> stub facade is active. `tools/dependency_registry` shells out to `cargo
> metadata` through the in-house JSON facade, dropping the crates.io
> `cargo_metadata`/`camino` pair while adding unit coverage for the parser and
> teaching integration tests to auto-skip on the stub backend.
> **Review (2025-10-14, evening):** Regression coverage now locks the freshly
> handwritten TLS serializers and the JSON facade. `cli/src/tls.rs` ships
> dedicated tests that round-trip `CliTlsWarningStatus`, snapshots, and the
> aggregated status report through `foundation_serialization::json` while
> asserting optional-field elision and unknown-field tolerance, preventing the
> manual deserializers from regressing. `crates/foundation_serialization/tests/
> json_value.rs` exercises nested objects, duplicate keys, and non-finite float
> rejection so the manual `Value` impl stays in parity with serde’s semantics,
> and `node/src/storage/pipeline/binary.rs` gained
> `write_field_count_rejects_overflow` to prove the cursor guard fires when the
> provider-profile encoder overflows. Together the suites keep the stub backend
> honest while FIRST_PARTY_ONLY CLI runs continue to pass against the in-house
> codec.
> **Review (2025-10-14, afternoon):** `foundation_serde` now mirrors serde’s
> visitor coverage for the TLS surfaces we rely on. The stub backend implements
> option/sequence/map/tuple/array handling, `foundation_serialization::json::Value`
> regained manual `Serialize`/`Deserialize` parity, and the CLI’s TLS structs
> (`CliTlsWarningStatus`, snapshots, origins, status reports, and certificate
> manifests) now ship handwritten serializers/deserializers that drop the legacy
> derive path entirely. `FIRST_PARTY_ONLY=0 cargo test -p contract-cli --lib`
> passes on the stub backend, exercising JSON round-trips for status, snapshot,
> and manifest payloads. Node defaults were tightened at the same time:
> aggregator/quic configs and storage engine selection now call the in-house
> default helpers directly, peer reputation records reuse the shared
> `instant_now()` guard, compute-offer telemetry normalises the reputation
> multiplier through a first-party helper, and the storage pipeline’s binary
> encoder checks field counts through the cursor stack so the overflow guard is
> actually exercised. The cleanup eliminates the lingering `unused` warnings in
> `node/src` and keeps FIRST_PARTY_ONLY checks noise-free while preserving the
> TLS automation workflow.
> **Review (2025-10-14, mid-morning):** Hardened terminal prompting across the
> stack. `sys::tty` now routes passphrase reads through a generic helper that
> unit tests exercise with in-memory streams, trimming carriage returns and
> guaranteeing echo guards run even when stdin is not a TTY. `foundation_tui`
> adds override hooks so CLI/tests can inject scripted responses without pulling
> in third-party prompt crates, and `contract-cli`’s log helpers gained unit
> tests that cover optional/required passphrase flows and whitespace handling.
> FIRST_PARTY_ONLY builds keep interactive commands functional while the new
> tests guard regressions.
> **Review (2025-10-14, late night):** Restored the runtime watcher modules on
> Linux and BSD to the first-party `sys::inotify` and `sys::kqueue` shims
> (`crates/runtime/src/fs/watch.rs`), reinstating recursive registration,
> overflow handling, and deregistration on drop through the in-house reactor.
> The Windows watcher now rides the IOCP-backed `DirectoryChangeDriver`
> (`crates/sys/src/fs/windows.rs`) with explicit `Send` guarantees so the
> blocking worker satisfies `spawn_blocking` bounds, and
> `crates/sys/Cargo.toml` pulls in the `foundation_windows` FFI bindings required for
> cross-target builds. `FIRST_PARTY_ONLY=1` checks for `sys`/`runtime` now pass
> on `x86_64-pc-windows-gnu`, closing the gap opened by the watcher rewrite.
> **Review (2025-10-14, evening):** The cross-platform runtime stack now spans
> Windows with first-party code and an IOCP-backed reactor. The new
> `crates/sys/src/reactor/platform_windows.rs` associates every socket with a
> completion port, shards WSA event waiters that post completions into the queue,
> and posts runtime wakers via `PostQueuedCompletionStatus`, eliminating the
> old 64-handle ceiling. `crates/sys/src/net/windows.rs` mirrors the Unix socket
> constructors via `WSASocketW` while implementing `AsRawSocket` so runtime
> abstractions can treat handles generically (`ReactorRaw`) across all targets.
> Regression coverage now includes a Windows scaling check
> (`crates/sys/tests/reactor_windows_scaling.rs`) alongside the UDP stress loop
> (`crates/sys/tests/net_udp_stress.rs`) and the existing TCP/Linux/BSD suites,
> keeping readiness semantics and ordering intact. Runtime’s Windows file
> watcher temporarily falls back to the polling stub, gated behind a
> `windows-fs-watcher` feature until the native directory loop lands, and docs +
> audits note the IOCP rollout, new `just check-windows` recipe, and CI cross-
> target checks.
> **Review (2025-10-14):** `crates/sys` now ships an epoll-backed `reactor`
> (`Poll`, `Events`, `Waker`), a fully in-house kqueue backend for
> macOS/BSD (including EVFILT_USER wakeups and descriptor registration), and
> fresh TCP/UDP constructors under `sys::net`, letting the runtime register
> descriptors and open sockets without touching third-party crates. The
> in-house backend wires those modules end to end: file watching, TCP
> listeners/streams, and UDP sockets all register through the first-party
> reactor, and the `runtime` crate’s `mio`, `nix`, and `socket2`
> dependencies disappeared. FIRST_PARTY_ONLY builds now compile the watcher
> and networking stacks exclusively against in-house code, the Linux
> integration suite (`crates/sys/tests/inotify_linux.rs`) continues to
> exercise create/delete/directory events, and new coverage hammers the
> kqueue reactor (`crates/sys/tests/reactor_kqueue.rs`, cfg’d for BSD) plus a
> 32-iteration TCP send/recv stress loop
> (`crates/sys/tests/net_tcp_stress.rs`) that guards the non-blocking socket
> wrappers and the EINPROGRESS handling added to
> `sys::net::TcpStream::connect`. The dependency inventory/audit notes reflect
> the slimmer graph alongside a TODO to retire the remaining `tokio` → `mio`
> edge.
> **Review (2025-10-12):** Runtime’s in-house backend now schedules async tasks
> and blocking jobs via a shared first-party `WorkQueue`, eliminating the
> `crossbeam-deque`/`crossbeam-epoch` dependency pair while keeping spawn
> latency and pending-task telemetry intact. `foundation_bigint` now ships a
> production-grade big-integer engine with an `tests/arithmetic.rs` suite that
> exercises addition, subtraction, multiplication, decimal/hex parsing,
> shifting, modular reduction, and modular exponentiation to keep the
> in-house implementation locked to deterministic vectors.
> **Review (2025-10-12):** Landed the `foundation_serde` facade and stub
> backend so every crate toggles serde usage through the first-party wrapper.
> Workspace manifests now alias `serde` to the facade, and `foundation_bigint`
> replaces the `num-bigint` stack inside `crypto_suite` so FIRST_PARTY_ONLY
> builds run without crates.io big integers while residual `num-traits` stays behind image/num-* tooling.
> `foundation_serialization` still owns mutually exclusive
> `serde-external`/`serde-stub` features, the stub mirrors serde trait surfaces
> (serializers, deserializers, visitors, value helpers, primitive
> implementations, and `IntoDeserializer` adapters), and FIRST_PARTY_ONLY
> builds compile with the stub via `cargo check -p foundation_serialization
> --no-default-features --features serde-stub`. Dependency inventory snapshots
> and the guard backlog were refreshed to capture the new facade boundaries.
> **Review (2025-10-13):** `foundation_serde_derive` now parses proc-macro
> input directly with `proc_macro` token trees, eliminating the
> `proc-macro2`/`quote`/`syn` stack while keeping stub derives for
> `Serialize`/`Deserialize`. The stub backend gained container coverage for
> vectors, tuples, hash maps, const arrays, and references so FIRST_PARTY_ONLY
> builds satisfy workspace trait bounds even when serde would normally generate
> blanket impls. A workspace-level `serde` alias now points at the facade and
> manifests across node, governance, wallet, explorer, CLI, light-client,
> telemetry, inflation, bridges, TLS, and tooling crates consume the shared
> entry to keep feature selection consistent between stub and external
> backends.
> **Review (2025-10-12):** Test infrastructure now compiles without the `syn`/`quote`/`proc-macro2` stack—`crates/testkit_macros` parses serial test wrappers directly and still guards execution behind `testkit::serial::lock()`. Foundation math tests rely on new in-house floating-point helpers (`testing::assert_close[_with]`), eliminating the external `approx` dependency. Wallet and remote-signer binaries removed the dormant `hidapi` feature flag so `FIRST_PARTY_ONLY` builds no longer link native HID toolchains; the Ledger placeholder still returns a deterministic error. Runtime and gateway code now share the `foundation_async` facade: `crates/runtime` re-exports the shared oneshot channel, the first-party `AtomicWaker` gained deferred-wake semantics, and coverage in `crates/foundation_async/tests/futures.rs` locks join ordering, select short-circuiting, panic capture, and cancellation paths. Dependency inventories and the first-party audit were refreshed to reflect the leaner workspace DAG.
> **Review (2025-10-14):** Price board persistence no longer relies on placeholder derives—the manual `Serialize`/`Deserialize` implementations in `node/src/compute_market/price_board.rs` now drive the facade directly, and a regression fixture (`PRICE_BOARD_FIXTURE`) keeps the binary contract stable across FIRST_PARTY_ONLY runs. Light-client state caching follows suit: `crates/light-client/src/state_stream.rs` implements first-party serialization for the persisted cache, exposes a mirror reference encoder, and locks the output through `PERSISTED_STATE_FIXTURE` so the binary decoder surfaces drift immediately. Both suites feed the new unit tests exercising encode/decode round-trips through `foundation_serialization::binary`, ensuring production persistence guards stay aligned with the in-house serializer.
> **Review (2025-10-14, evening):** Added FIRST_PARTY_ONLY smoke tests to the price board and light-client fixtures so encode/decode coverage now forces the guard to `1`, `0`, and unset; all three paths produce identical bytes, confirming guarded CI runs and local unstaged workflows observe the same persistence contract.
> **Review (2025-10-14, late):** Storage provider profile cursors (`storage/pipeline/binary.rs`) and DID registry snapshots (`identity/did_binary.rs`) ship deterministic fixtures guarded by FIRST_PARTY_ONLY parity tests, keeping sled persistence byte-stable with the guard forced to `1`, `0`, or unset while retaining the legacy compatibility harnesses.
> **Review (2025-10-13):** Metrics aggregation now installs a first-party `AggregatorRecorder` so every `foundation_metrics` macro emitted by runtime backends, TLS sinks, and CLI tools flows back into the Prometheus handles without reintroducing third-party crates. Monitoring utilities gained a lightweight `MonitoringRecorder` that keeps snapshot success/error counters inside the facade, letting the `snapshot` CLI report health without depending on the retired `metrics` stack. Gateway read receipts now bypass serde, encoding and decoding through the new `foundation_serialization::binary_cursor` helpers while retaining the legacy CBOR fallback for older batches. Gossip wire messages follow suit: `node/src/p2p/wire_binary.rs` replaces the serde-derived `WireMessage` encoder/decoder with cursor helpers plus upgraded `binary_struct` guards, and regression tests lock the legacy payload bytes across handshake and gossip variants. Storage sled codecs—rent escrow, manifests, provider profiles, and repair failure records—now share the cursor helpers (`node/src/storage/{fs.rs,manifest_binary.rs,pipeline/binary.rs,repair.rs}`) with expanded regression suites that exercise large manifests, redundancy variants, and historical payloads lacking optional fields, and the new randomized property harness plus sparse-manifest repair integration test keep the first-party codecs in parity with the retired binary shim. Identity DID and handle registries likewise persist through `identity::{did_binary,handle_binary}`, replacing `binary_codec` in sled storage while compatibility fixtures guard remote attestation, pq-key toggles, and truncated payloads, and freshly added seeded property tests plus the `identity_snapshot` integration suite stress randomized identities alongside mixed legacy/current sled dumps. DEX persistence now joins the cursor stack: `node/src/dex/{storage.rs,storage_binary.rs}` encode order books, trade logs, AMM pools, and escrow snapshots via first-party helpers while the new `EscrowSnapshot` type documents the persisted layout and regression suites (`order_book_matches_legacy`, `trade_log_matches_legacy`, `escrow_state_matches_legacy`, `pool_matches_legacy`) lock legacy bytes. The PQ surface now rides on first-party stubs (`crates/pqcrypto_dilithium`, `crates/pqcrypto_kyber`), allowing `quantum` and `pq` builds to generate keys, signatures, and encapsulations without crates.io code while keeping deterministic encodings for commit–reveal, wallet, and governance tests. Byte-array helpers have moved in-house as well: the workspace drops the external `serde_bytes` crate in favour of `foundation_serialization::serde_bytes`, so `#[serde(with = "serde_bytes")]` annotations continue to compile when the FIRST_PARTY_ONLY guard is active.
> **Review (2025-10-12):** Storage engine manifests and WAL snapshots now round-trip through the new in-house JSON codec and temp-file harness, eliminating the crate’s `foundation_serialization`/`serde`/`sys::tempfile` dependencies and adding regression tests for malformed input, unicode escapes, byte-array coercions, and persist failures. The diagnostics crate no longer links the SQLite facade, `dependency_guard` scopes `cargo metadata` to the requesting crate before enforcing policy, and the dependency inventory snapshots were regenerated to reflect the leaner workspace DAG. Node telemetry now rides on the first-party `foundation_metrics` recorder, bridging runtime spawn-latency histograms, pending-task gauges, and wallet/CLI counters into the existing telemetry surfaces without touching the retired `metrics` crate.
> **Prior update (2025-10-11):** FIRST_PARTY_ONLY transport builds drop the s2n feature, the in-house QUIC certificate store persists DER material with corruption pruning and relocation-friendly overrides, and `transport_quic` routes provider selection through the first-party adapter while surfacing provider identifiers to gossip handshakes. CLI, explorer, wallet, and support tooling now emit `diagnostics::TbError` instead of `anyhow`, the simulation harness writes dashboards via a first-party CSV emitter, and the remote signer trace IDs derive from an in-house generator so the workspace drops the `uuid` crate entirely. Follow-up work removes the `assert_cmd` / `predicates` dev stack from the `xtask` lint harness in favour of standard library process helpers, adds regression coverage that locks the new trace ID format, and refreshes `docs/dependency_inventory.{md,json}` plus the violations snapshot to excise `anyhow`, `csv`, `uuid`, `assert_cmd`, and `predicates` from the manifest.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding,
> crypto_suite, codec, serialization, SQLite, diagnostics, TUI, TLS, and HTTP env facades are
> live with governance overrides enforced (2025-10-12); node, telemetry, and
> harness tooling now default to the first-party binary codec.

This document tracks high‑fidelity progress across The‑Block's major work streams.  Each subsection lists the current completion estimate, supporting evidence with canonical file or module references, and the remaining gaps.  Percentages are rough, *engineer-reported* gauges meant to guide prioritization rather than marketing claims.

Mainnet readiness currently measures **98.3/100** with vision completion **93.3/100**. Subsidy accounting now lives solely in the unified CT ledger; see `docs/system_changes.md` for migration notes. The standalone `governance` crate mirrors the node state machine for CLI/SDK use, the compute marketplace enforces lane-aware batching with fairness deadlines, starvation telemetry, and per-lane persistence, the mobile gateway cache persists encrypted responses with TTL hygiene plus CLI/RPC/telemetry visibility, wallet binaries share the crypto suite’s first-party Ed25519 backend with multisig signer telemetry, the RPC client clamps `TB_RPC_FAULT_RATE` while saturating exponential backoff, overlay discovery/uptime/persistence flow through the trait-based `p2p_overlay` crate with in-house and stub backends, the storage engine abstraction unifies RocksDB, sled, and memory providers via `crates/storage_engine`, the coding crate gates XOR parity and RLE compression fallbacks behind audited rollout policy while tagging storage telemetry and powering the bench-harness comparison mode, the gossip relay couples an LRU-backed dedup cache with adaptive fanout and partition tagging, the proof-rebate tracker persists receipts that land in coinbase assembly with explorer/CLI pagination, wrapper telemetry exports runtime/transport/storage/coding/codec/crypto metadata through both node metrics and the aggregator `/wrappers` endpoint, release provenance now hashes the vendored tree while recording dependency snapshots enforced by CI, CLI, and governance overrides, and the runtime-backed HTTP client now covers node/CLI surfaces while the gateway/status servers and explorer run on the in-house httpd router alongside an indexer CLI migrated to `cli_core` + httpd. The dependency-sovereignty pivot is documented in [`docs/pivot_dependency_strategy.md`](pivot_dependency_strategy.md) and reflected across every subsystem guide. Remaining focus areas: extend bridge/DEX docs with signer-set payloads and release-verifier guidance, integrate compute-market SLA alerts with the aggregator dashboards, continue WAN-scale QUIC chaos drills, polish multisig UX, and retire the remaining clap-derived simulation harness now that node, contract, and tooling CLIs run on `cli_core` plus the JSON codec.

**New (2025-10-20):** Node runtime logging and governance webhooks now serialize via explicit first-party builders, retiring the
`foundation_serialization::json!` macro from production binaries. The CLI log sink’s stderr/trace emitters and the governance
webhook client share deterministic `JsonMap` assembly backed by regression tests (`node/src/bin/node.rs`, `node/src/telemetry.rs`).

CLI, node, light-client, and metrics-aggregator binaries now build exclusively on the runtime facade’s in-house backend; Tokio persists only as a dormant compatibility feature inside `crates/runtime` while the remaining tooling CLIs finish migrating off their bespoke HTTP stacks (production HTTP services already run on `httpd`).

\[
\text{multiplier}_x = \frac{\phi_x I_{\text{target}} S / 365}{U_x / \text{epoch\_secs}}
\]

clamped to ±15 % of the previous value. Base miner rewards decrease as the effective miner count rises following

\[
R_0(N) = \frac{R_{\max}}{1 + e^{\xi (N - N^\star)}}
\]

with hysteresis `ΔN ≈ √N*` to blunt flash joins. Full derivations live in [`docs/economics.md`](economics.md). The canonical roadmap with near‑term tasks lives in [`docs/roadmap.md`](roadmap.md).

## Dependency posture

- **Policy source**: [`config/dependency_policies.toml`](../config/dependency_policies.toml) enforces a depth limit of 3, assigns risk tiers, and blocks AGPL/SSPL transitively.  The registry snapshot is materialised via `cargo run -p dependency_registry -- --check config/dependency_policies.toml` and stored at [`docs/dependency_inventory.json`](dependency_inventory.json).
- **Current inventory** *(generated at `2025-10-14T17:20:00Z`)*: 0 strategic crates, 0 replaceable crates, and 0 unclassified dependencies in the resolved workspace DAG. The new snapshot captures the `foundation_fuzz` rollout, the retirement of the external QR/serde backends, and the first-party sled legacy shim.
- **Outstanding drift**: 0 — the dependency inventory and violations report are empty now that the sled legacy importer is first-party. CI still publishes the registry/violations bundle each pull request to catch regressions immediately.
- **Latest migrations (2025-10-14)**: `foundation_fuzz` replaces the libFuzzer
  bridge, the net/gateway harnesses reuse the shared modules and ship smoke
  coverage, `foundation_qrcode` ships as a pure in-house backend,
  `foundation_serde` drops the external escape hatch, and the sled
  `legacy-format` importer now reads JSON manifests through first-party code—
  removing `libfuzzer-sys`, `arbitrary`, `qrcode`, the upstream `serde` stack,
  and the crates.io `sled` cluster from the workspace. Remote-signer now emits
  QR codes solely through the stub facade. `tools/xtask` plus
  `scripts/release_provenance.sh` parse/hash the summary so CI preflights and
  release artefacts surface the drift verdict with matching checksums.
  Monitoring’s metric catalogue, Grafana templates, and alert rules now expose
  dependency policy status, drift counts, and snapshot freshness, with
  regenerated dashboards and snapshot fixtures keeping build/test coverage
  aligned.
- **Latest migrations (2025-10-14, evening++)**: Retired the third-party
  `tracing` stack from wallet and light-client builds in favour of
  `diagnostics::tracing`, introduced the `foundation_qrcode` facade with a
  first-party QR stub for the remote-signer CLI, and dropped the unused
  `static_assertions` crate from the node manifest/first-party manifest. The
  guard backlog now focuses on migrating residual tooling to `foundation_windows`.
- **Latest migrations (2025-10-14, late)**: `crates/coding` removed the
  `allow-third-party` escape hatch; the property harness now runs entirely on the
  workspace RNG while the LT fountain coder encodes/decodes through the
  first-party Reed–Solomon backend (`crates/coding/src/fountain/inhouse.rs`),
  replacing the "requires RNG" stub. `crates/rand` picked up deterministic
  `fill`, `choose[_mut]`, and slice sampling helpers with dedicated tests
  (`crates/rand/tests/seq.rs`), and simulation tooling (`sim/did.rs`) switched to
  the new APIs so account rotation stays first party without manual indexing.
- **Latest migrations (2025-10-14, final pass)**: `crates/rand` now samples
  `u64`/`usize`/`i64` ranges via rejection sampling so large domains avoid modulo
  bias; range tests (`crates/rand/tests/range.rs`) cover tail-heavy spans and the
  full signed range. The fountain harness picked up parity-budget and burst-loss
  regression tests to validate the new LT implementation under systematic
  packet drops. `tools/xtask` dropped the `--allow-third-party` escape hatch, so
  dependency audits always run with `FIRST_PARTY_ONLY` enforcement.
- **Latest migrations (2025-10-12)**: Runtime rewired its async executor and
  blocking pool to use a shared first-party `WorkQueue`, dropping
  `crossbeam-deque`/`crossbeam-epoch` from the crate while preserving spawn
  latency/pending task gauges. `foundation_bigint` picked up deterministic
  arithmetic/parsing/modpow tests that run under both stub and external
  backends so FIRST_PARTY_ONLY builds validate the new facade in isolation.
- **Latest migrations (2025-10-13)**: Gossip wire messages now encode/decode
  via `node/src/p2p/wire_binary.rs`, replacing the serde-derived
  `WireMessage` codec with cursor helpers, compatibility fixtures, and a new
  invalid-value guard inside `binary_struct`. Gateway read receipts now encode
  and decode via the first-party `foundation_serialization::binary_cursor`
  helpers, eliminating the serde derive in that path while retaining the
  legacy CBOR fallback for historical payloads. The storage engine now
  serializes
  manifests/WALs via `crates/storage_engine::json` and isolates temp-dir usage
  behind `crates/storage_engine::tempfile`, removing `foundation_serialization`,
  `serde`, and `sys::tempfile` from the crate and adding regression coverage for
  malformed values and persist failures. The diagnostics crate dropped its
  `foundation_sqlite` dependency, `dependency_guard` scopes `cargo metadata` to
  the requesting crate before enforcing policy, and QUIC transport caches now
  ride on the first-party `concurrency::DashMap`, removing the external
  `dashmap` crate. `foundation_tls` builds without `rustls` (session caching
  lives beside the providers in `crates/transport`), and the s2n backend
  verifies certificates through the new in-house DER parser
  (`crates/transport/src/cert_parser.rs`), replacing `x509-parser` entirely.
- **Latest migrations (2025-10-14, late night+)**: `tools/dependency_registry` now parses
  policy TOML via the facade’s new low-level helpers and serializes snapshots
  with handwritten JSON conversions, removing the crate’s last serde dependency.
  Tests execute under the stub backend without skips, and
  `crates/foundation_serialization/tests/toml_policy.rs` locks the parser
  against regression inputs while `json::to_vec_value` powers CLI outputs.
- **Latest migrations (2025-10-14)**: The new `crates/log_index` library
  replaces the ad-hoc SQLite helpers with a sled-backed store shared by node,
  CLI, explorer, and monitoring tooling. `contract logs` and the
  `log-indexer` CLI now ingest, search, and rotate keys through the crate while
  telemetry observers emit ingestion counters per correlation ID. The optional
  `sqlite-migration` feature only gates legacy imports, so default builds stay
  first party end-to-end. Targeted regression suites cover plaintext,
  encrypted, and rotation paths, skipping automatically when the
  `foundation_serde` stub backend is selected.
- Newly migrated storage sled codecs in
  `node/src/storage/{manifest_binary.rs,pipeline/binary.rs,fs.rs,repair.rs}`
  replace serde/binary-codec persistence with cursor helpers, broaden
  compatibility coverage (large manifests, `Redundancy::None`, sparse provider
  tables), and add decode tests that tolerate historical payloads lacking the
  modern optional fields.
- Newly migrated DEX sled codecs in
  `node/src/dex/{storage.rs,storage_binary.rs}` remove the `binary_codec`
  shim, encode order books, AMM pools, trade logs, and escrow snapshots via the
  cursor helpers, and add randomized/legacy regression suites that validate the
  new layouts while documenting the persisted `EscrowSnapshot` schema.
- Newly migrated identity sled registries in
  `node/src/identity/{did_binary.rs,handle_binary.rs}` and their callers replace
  `binary_codec` persistence with cursor helpers, preserve remote-attestation and
  pq-key toggles via compatibility suites, add seeded parity/fuzz suites for DID
  and handle records, and feed the DID/handle stores used by CLI, explorer, and
  governance revocation flows while the `identity_snapshot` integration test
  verifies mixed legacy/current sled dumps.
- ✅ `metrics-aggregator` now installs the in-house `AggregatorRecorder` so
  `foundation_metrics` macros emitted across runtime, TLS, and tooling sinks
  flow back into the Prometheus registry without regressing integer TLS
  fingerprints or spawn-latency histograms.
- ✅ The monitoring snapshot tooling installs a lightweight `MonitoringRecorder`
  that tracks success/error totals through the same facade, allowing the
  `snapshot` CLI to report health without the retired third-party `metrics`
  crate.
- ✅ Error handling across CLI, explorer, wallet, and tooling now routes through
  `diagnostics`, removing the third-party `anyhow` facade while keeping context
  helpers intact.
- ✅ `tb-sim` exports CSV dashboards via a small in-house writer, removing the
  external `csv` crate without changing downstream automation inputs.
- ✅ Wallet remote signer trace identifiers now come from a first-party
  generator so the workspace no longer links the crates.io `uuid` crate.
- FIRST_PARTY_ONLY transport builds now exclude the s2n feature; the in-house
  certificate store persists DER blobs and `node::net::transport_quic` selects
  providers dynamically, keeping QUIC handshakes on first-party code paths even
  when Quinn/s2n are unavailable.
- Target-specific dependency gating now disables the Quinn feature whenever
  `FIRST_PARTY_ONLY` is set, letting the transport crate compile with only the
  in-house and s2n adapters while keeping standard builds on Quinn+s2n.
- **Next refresh**: Run `./scripts/dependency_snapshot.sh` on **2025-10-13** once the remaining tooling migrations (monitoring dashboards, remote signer) land to capture the cleaned DAG and refresh these metrics.
- **In-house scaffolding**: Bootstrapped the `diagnostics` error/logging facade and the `concurrency` primitive crate to replace third-party `anyhow`/`tracing`/`log`/`dashmap` usages; `dependency_registry` is now wired to the new stack while we phase in the remaining migrations. Added a workspace-local `rand` crate over the stubbed `rand_core` module so all binaries compile against first-party randomness helpers, routed CLI/light-client/transport home-directory lookups through the new `sys::paths` adapters to remove the `dirs` dependency, introduced the `foundation_sqlite` facade so optional SQLite tooling now compiles against first-party parameter/value handling before the native engine ships, landed `foundation_time` so S3 signing, storage repair logs, and QUIC certificate rotation rely on our in-house timestamp helpers, delivered `foundation_unicode` so identity tooling no longer depends on ICU, shipped `foundation_tls`/`foundation_tui` so QUIC certificates and CLI colour output are fully first party, rewrote `tools/xtask` to call the git CLI directly so the `git2`/`url`/`idna` stack disappears from the workspace DAG, and added the `http_env` crate to centralise TLS environment parsing for clients while emitting component-scoped fallbacks.
- ✅ `foundation_sqlite` now persists its in-memory tables via `database_to_json`/`database_from_json`, replacing the temporary binary shim. Conflict resolution, ORDER BY/LIMIT clauses, LIKE filters, and provider join emulation are locked down through new unit tests (`cargo test -p foundation_sqlite`), ensuring explorer/indexer imports ride first-party JSON without serde derives.
- **TLS automation**: Added the `tls-manifest-guard` helper and wired it into the systemd units so manifests, environment exports, and renewal windows are validated before reloads. Metrics ingestion now forwards `tls_env_warning_total{prefix,code}` deltas from nodes into the aggregator, stamps `tls_env_warning_last_seen_seconds{prefix,code}` from the shared sink, rehydrates warning freshness from node-exported gauges after restarts, and respects the configurable `AGGREGATOR_TLS_WARNING_RETENTION_SECS` window. Nested telemetry encodings are covered by integration tests, and fleet dashboards (including the auto-generated templates) ship panels plus a `TlsEnvWarningBurst` alert sourcing the same counter/gauge pair. The guard now carries fixture-driven tests for stale reminders and env exports to block regressions in CI, emits machine-readable summaries with `--report <path>`, enforces that staged files live under the declared directory and that env exports use the manifest prefix, and warns when the env file carries extra prefix-matching exports. The aggregator exposes `/tls/warnings/latest` so operators can pull structured `{prefix,code}` diagnostics without scraping logs and ships an end-to-end test that spins up the HTTP service to prove sink fan-out and peer ingests both update `/metrics` and `/tls/warnings/latest`. Fingerprint gauges (`tls_env_warning_detail_fingerprint{prefix,code}`, `tls_env_warning_variables_fingerprint{prefix,code}`) and counters (`tls_env_warning_detail_fingerprint_total{prefix,code,fingerprint}`, `tls_env_warning_variables_fingerprint_total{prefix,code,fingerprint}`) now hash warning payloads so dashboards correlate variants without free-form detail strings, aggregator snapshots track per-fingerprint counts, `tls_env_warning_events_total{prefix,code,origin}` exposes diagnostics-versus-peer deltas, the shared `crates/tls_warning` module unifies BLAKE3 hashing for every consumer, and `contract telemetry tls-warnings` adds `--probe-detail` / `--probe-variables` helpers for local fingerprint calculations.
- **TLS automation**: Added the `tls-manifest-guard` helper and wired it into the systemd units so manifests, environment exports, and renewal windows are validated before reloads. Metrics ingestion now forwards `tls_env_warning_total{prefix,code}` deltas from nodes into the aggregator, stamps `tls_env_warning_last_seen_seconds{prefix,code}` from the shared sink, rehydrates warning freshness from node-exported gauges after restarts, and respects the configurable `AGGREGATOR_TLS_WARNING_RETENTION_SECS` window. Nested telemetry encodings are covered by integration tests, and fleet dashboards (including the auto-generated templates) ship panels plus a `TlsEnvWarningBurst` alert sourcing the same counter/gauge pair. The guard now carries fixture-driven tests for stale reminders and env exports to block regressions in CI, emits machine-readable summaries with `--report <path>`, enforces that staged files live under the declared directory and that env exports use the manifest prefix, and warns when the env file carries extra prefix-matching exports. The aggregator exposes `/tls/warnings/latest` so operators can pull structured `{prefix,code}` diagnostics without scraping logs and ships an end-to-end test that spins up the HTTP service to prove sink fan-out and peer ingests both update `/metrics` and `/tls/warnings/latest`. Fingerprint gauges (`tls_env_warning_detail_fingerprint{prefix,code}`, `tls_env_warning_variables_fingerprint{prefix,code}`) and counters (`tls_env_warning_detail_fingerprint_total{prefix,code,fingerprint}`, `tls_env_warning_variables_fingerprint_total{prefix,code,fingerprint}`) now hash warning payloads so dashboards correlate variants without free-form detail strings, aggregator snapshots track per-fingerprint counts, `tls_env_warning_events_total{prefix,code,origin}` exposes diagnostics-versus-peer deltas, the shared `crates/tls_warning` module unifies BLAKE3 hashing for every consumer, and `contract telemetry tls-warnings` adds `--probe-detail` / `--probe-variables` helpers for local fingerprint calculations.
 Telemetry refresh: the shared sink now installs a diagnostics subscriber that mirrors `TLS_ENV_WARNING` log lines into the same pipeline without double-counting counters so `tls_env_warning_last_seen_seconds{prefix,code}` keeps advancing even when only observers fire, aggregator fingerprint ingestion decodes JSON numbers and hex labels into exact 64-bit integers (eliminating the f64 rounding collisions that previously caused CLI/Prometheus mismatches), and the monitoring snapshot plus `compare_tls_warnings` helper ingest typed `MetricValue::{Float,Integer,Unsigned}` entries instead of lossy `f64`s when cross-checking totals and per-fingerprint counters. The node exposes `ensure_tls_env_warning_diagnostics_bridge()` so diagnostics-only pipelines feed the same metrics even without registered sinks, and ships `reset_tls_env_warning_forwarder_for_testing()` for repeatable integration harnesses.
  Rust consumers can now call `tls_warning::register_tls_env_warning_telemetry_sink()` (or the re-exported `the_block::telemetry::register_tls_env_warning_telemetry_sink()`) to stream `TlsEnvWarningTelemetryEvent` payloads (prefix, code, origin, totals, last-seen timestamp, hashed detail/variable buckets, and change flags) directly into dashboards or tooling, with guard-based unregistration to avoid leaking handlers. Test harnesses clear the shared registry with `tls_warning::reset_tls_env_warning_telemetry_sinks_for_test()` before installing bespoke callbacks.
  Fingerprint gauges now register as integer metrics so Prometheus samples preserve every bit of the BLAKE3 digest, and the CLI’s `contract telemetry tls-warnings` table includes an `ORIGIN` column that mirrors the `tls_env_warning_events_total{prefix,code,origin}` label set.
- **TLS automation**: Added the `tls-manifest-guard` helper and wired it into the systemd units so manifests, environment exports, and renewal windows are validated before reloads. Metrics ingestion now forwards `tls_env_warning_total{prefix,code}` deltas from nodes into the aggregator, stamps `tls_env_warning_last_seen_seconds{prefix,code}` from the shared sink, rehydrates warning freshness from node-exported gauges after restarts, and respects the configurable `AGGREGATOR_TLS_WARNING_RETENTION_SECS` window. Nested telemetry encodings are covered by integration tests, and fleet dashboards (including the auto-generated templates) ship panels plus a `TlsEnvWarningBurst` alert sourcing the same counter/gauge pair. The guard now carries fixture-driven tests for stale reminders and env exports to block regressions in CI, emits machine-readable summaries with `--report <path>`, enforces that staged files live under the declared directory and that env exports use the manifest prefix, and warns when the env file carries extra prefix-matching exports. The aggregator exposes `/tls/warnings/latest` so operators can pull structured `{prefix,code}` diagnostics without scraping logs and ships an end-to-end test that spins up the HTTP service to prove sink fan-out and peer ingests both update `/metrics` and `/tls/warnings/latest`. Fingerprint gauges (`tls_env_warning_detail_fingerprint{prefix,code}`, `tls_env_warning_variables_fingerprint{prefix,code}`) and counters (`tls_env_warning_detail_fingerprint_total{prefix,code,fingerprint}`, `tls_env_warning_variables_fingerprint_total{prefix,code,fingerprint}`) now hash warning payloads so dashboards correlate variants without free-form detail strings, aggregator snapshots track per-fingerprint counts, `tls_env_warning_events_total{prefix,code,origin}` exposes diagnostics-versus-peer deltas, the shared `crates/tls_warning` module unifies BLAKE3 hashing for every consumer, and `contract telemetry tls-warnings` adds `--probe-detail` / `--probe-variables` helpers for local fingerprint calculations. Unique fingerprint gauges (`tls_env_warning_detail_unique_fingerprints{prefix,code}`, `tls_env_warning_variables_unique_fingerprints{prefix,code}`) expose how many hashed variants have appeared, the aggregator logs a structured "observed new tls env warning … fingerprint" entry whenever a non-`none` hash first arrives, CLI output prints per-fingerprint tallies, and `/export/all` bundles `tls_warnings/latest.json` plus `tls_warnings/status.json` so offline investigations keep hashed payloads and retention metadata with the metrics snapshot.
- **TLS automation** *(continued)*: Grafana now renders hashed fingerprint, unique-fingerprint, and five-minute delta panels for the TLS row so rotations can watch `tls_env_warning_*_fingerprint`/`tls_env_warning_*_fingerprint_total` without custom queries, and Prometheus ships `TlsEnvWarningNewDetailFingerprint`, `TlsEnvWarningNewVariablesFingerprint`, `TlsEnvWarningDetailFingerprintFlood`, and `TlsEnvWarningVariablesFingerprintFlood` alerts to escalate brand-new hashes or sustained bursts. The `monitoring compare-tls-warnings` helper cross-checks `contract telemetry tls-warnings --json` with `/tls/warnings/latest` and the Prometheus series, emitting labeled mismatches and a non-zero exit code when aggregator totals fall behind local snapshots.
- **TLS automation**: Added the `tls-manifest-guard` helper and wired it into the systemd units so manifests, environment exports, and renewal windows are validated before reloads. Metrics ingestion now forwards `tls_env_warning_total{prefix,code}` deltas from nodes into the aggregator, stamps `tls_env_warning_last_seen_seconds{prefix,code}` from the shared sink, rehydrates warning freshness from node-exported gauges after restarts, and respects the configurable `AGGREGATOR_TLS_WARNING_RETENTION_SECS` window. Nested telemetry encodings are covered by integration tests, and fleet dashboards (including the auto-generated templates) ship panels plus a `TlsEnvWarningBurst` alert sourcing the same counter/gauge pair. The guard now carries fixture-driven tests for stale reminders and env exports to block regressions in CI, emits machine-readable summaries with `--report <path>`, enforces that staged files live under the declared directory and that env exports use the manifest prefix, and warns when the env file carries extra prefix-matching exports. The aggregator exposes `/tls/warnings/latest` so operators can pull structured `{prefix,code}` diagnostics without scraping logs and ships an end-to-end test that spins up the HTTP service to prove sink fan-out and peer ingests both update `/metrics` and `/tls/warnings/latest`. Nodes now persist a local snapshot map (total, last delta, last seen, detail, variables, fingerprints, and per-fingerprint counts) behind `telemetry::tls_env_warning_snapshots()` with reset helpers for tests, and the CLI’s `contract telemetry tls-warnings` subcommand surfaces the same data alongside the new fingerprint probe options. The unique fingerprint gauges/logs and support-bundle exports described above apply equally to the node-local view, so on-host inspectors and offline bundles retain hashed payload counts and retention metadata.
-  `/tls/warnings/status` now reports retention health (`retention_seconds`, snapshot counts, and stale entries) alongside the latest structured payloads, Grafana adds a "TLS env warnings (age seconds)" panel that visualises `clamp_min(time() - max by (prefix, code)(tls_env_warning_last_seen_seconds), 0)` so rotation playbooks can verify warning freshness at a glance, the aggregator exports matching gauges (`tls_env_warning_retention_seconds`, `tls_env_warning_active_snapshots`, `tls_env_warning_stale_snapshots`, `tls_env_warning_most_recent_last_seen_seconds`, `tls_env_warning_least_recent_last_seen_seconds`) for Prometheus, monitoring ships the `TlsEnvWarningSnapshotsStale` alert, the `contract telemetry tls-warnings` subcommand mirrors the node snapshot view locally (with JSON and label filters) for on-host triage, and the existing `contract tls status --aggregator … --latest` helper renders both the status payload and most recent warning snapshots with remediation guidance.
- **Tooling migrations**: Removed the direct `serde`/`chrono` dependencies from `analytics_audit` and `dependency_registry`, routing derives through `foundation_serialization` and timestamps through `foundation_time` to keep the remaining tooling stack first party. The simulation harness now ships its governance scenario as JSON and parses it through `foundation_serialization::json`, retiring the lingering `serde_yaml` dependency from the workspace manifest.
- **Dependency guard rollout**: Extracted the build-script enforcement logic into `crates/dependency_guard` and wired every crate that still pulls from crates.io or git sources through the shared helper. The guard now fires for `cargo check -p <crate>` across CLI, metrics, storage, state, and tooling crates, with `.cargo/config.toml` defaulting `FIRST_PARTY_ONLY=1` and documenting the explicit `FIRST_PARTY_ONLY=0` escape hatch for staged rewrites.
- **State serialization rewrite**: Replaced the `state` crate’s reliance on `serde`, `serde_json`, and `bincode` with compact first-party encoders. Snapshots, contract stores, schema markers, and audit helpers now emit deterministic binary or JSON strings without touching third-party codecs, unblocking the dependency freeze for the state pipeline.
- **Base64 replacement**: Introduced the `base64_fp` crate and switched CLI, node networking, storage snapshots, transport certificate persistence, wallet remote signer flows, and tooling over to the first-party encoder/decoder so no workspace crate pulls the crates.io `base64` API directly; remaining third-party usage now exists only transitively and is earmarked for upcoming vendor rewrites.
- **Base58 replacement**: Swapped overlay peer persistence and supporting tooling to the first-party `foundation_serialization::base58` helpers, allowing removal of the crates.io `bs58` dependency from the workspace DAG.
- **Histogram rewrite**: Removed the `hdrhistogram` dependency by adding the `histogram_fp` crate and porting telemetry memory sampling to the in-house implementation; operators now rely solely on first-party percentile tracking while the richer feature set is rebuilt.
- **Installer update stub**: Removed the `self_update`/`reqwest` stack from the installer tool, replacing it with a temporary in-house stub that instructs operators to fetch releases manually until the dedicated updater lands, eliminating another base64 transitively pulled from crates.io.
- **DKG transition**: Replaced the external `threshold_crypto` crate with a temporary first-party implementation (`dkg/src/lib.rs`) so the distributed key-generation surface compiles without third-party code while the permanent scheme is built out. The dependency snapshot will be regenerated once the remaining first-party swaps land.
- **Python bridge stub**: Retired the PyO3 bindings and introduced the `python_bridge` facade that currently returns feature-disabled errors unless the forthcoming `python-bindings` feature ships, keeping demo scripts and CI shims honest about the missing FFI.
- **Testkit macros**: Added the `testkit` crate and its `testkit_macros` companion to replace Criterion, Proptest, Insta, SerialTest, and friends. The refreshed `tb_bench!`, `tb_prop_test!`, `tb_snapshot_test!`, `tb_snapshot!`, `tb_fixture!`, and `tb_serial` macros now execute first-party harnesses—benchmarks report timing summaries, property suites run through the deterministic runner and in-house PRNG, snapshots persist under `tests/snapshots/`, fixtures return reusable wrappers, and serial tests lock a global mutex for isolation. Coverage no longer relies on external tooling.

## 1. Consensus & Core Execution — 93.6 %

**Evidence**
- Hybrid PoW/PoS chain: `node/src/consensus/pow.rs` embeds PoS checkpoints and `node/src/consensus/fork_choice.rs` prefers finalized chains.
- Kalman-weighted multi-window difficulty retune with `retune_hint` metrics in `node/src/consensus/difficulty_retune.rs` and `docs/difficulty.md`.
- Rollback checkpoints and partition recovery hooks in `node/src/consensus/fork_choice.rs` and `node/tests/partition_recovery.rs`.
- EIP‑1559 base fee tracker: `node/src/fees.rs` adjusts per block and `node/tests/base_fee.rs` verifies target fullness tracking.
- Adversarial rollback tests in `node/tests/finality_rollback.rs` assert ledger state after competing forks.
- Coinbase assembly applies proof rebates via `node/src/blockchain/process.rs::apply_rebates`, with restart/reorg coverage in `node/tests/light_client_rebates.rs`.
- Pietrzak VDF with timed commitment and delayed preimage reveal (`node/src/consensus/vdf.rs`, `node/tests/vdf.rs`) shrinks proofs and blocks speculative computation.
- Hadamard–Unruh committee sampler with Count-Sketch top‑k (`node/src/consensus/hadamard.rs`, `node/src/consensus/committee/topk.rs`).
- Sequential BLAKE3 proof-of-history ticker with optional GPU offload (`node/src/poh.rs`, `node/tests/poh.rs`). See `docs/poh.md`.
- Dilithium-based commit–reveal path with nonce replay protection (`node/src/commit_reveal.rs`, `node/tests/commit_reveal.rs`) compresses blind signatures and thwarts mempool DoS. See `docs/commit_reveal.md` for design details.
- Heisenberg + VDF fuse (`node/src/consensus/vdf.rs`) enforces a ≥2-block delay before randomness-dependent transactions execute.
- Parallel executor and transaction scheduler document concurrency guarantees (`docs/scheduler.md`, `node/src/parallel.rs`, `node/src/scheduler.rs`).
- Transaction lifecycle, memo handling, and dual fee lanes documented in `docs/transaction_lifecycle.md`.
- Macro-block checkpointing and per-shard fork choice preserve cross-shard ordering (`node/src/blockchain/macro_block.rs`, `node/src/blockchain/shard_fork_choice.rs`).

**Gaps**
- Formal safety/liveness proofs under `formal/` still stubbed.
- No large‑scale network rollback simulation.

## 2. Networking & Gossip — 98.4 %

**Evidence**
- Runtime-owned TCP/UDP reactor now backs the node RPC client/server plumbing (`crates/runtime/src/net.rs`, `node/src/rpc/client.rs`) and the gateway/status HTTP services. Buffered IO helpers live in `crates/runtime/src/io.rs` with integration coverage in `crates/runtime/tests/net.rs`.
- The `sys` reactor now covers both epoll and kqueue backends: `crates/sys/src/reactor/platform.rs` handles Linux via epoll while `crates/sys/src/reactor/platform_bsd.rs` drives EV_SET/EVFILT_USER paths for macOS/BSD. Linux integration remains in `crates/sys/tests/inotify_linux.rs`, BSD-specific coverage lives in `crates/sys/tests/reactor_kqueue.rs` (cfg’d), and the TCP harness `crates/sys/tests/net_tcp_stress.rs` hammers 32 non-blocking connect/accept/send/recv loops alongside the EINPROGRESS-safe handshake added to `crates/sys/src/net/unix.rs`.
- Deterministic gossip with partition tests: `node/tests/net_gossip.rs` and docs in `docs/networking.md`.
- QUIC transport with mutual-TLS certificate rotation, cached diagnostics, TCP fallback, provider introspection, and mixed-transport fanout; integration covered in `node/tests/net_quic.rs`, `crates/transport/src/lib.rs`, `crates/transport/src/quinn_backend.rs`, `crates/transport/src/s2n_backend.rs`, and `docs/quic.md`, with telemetry via `quic_cert_rotation_total`, `quic_provider_connect_total{provider}`, and per-peer `quic_retransmit_total`/`quic_handshake_fail_total` counters.
- In-house transport cache honours `TransportConfig.certificate_cache` overrides,
  prunes corrupt DER blobs, and ships a guard in `node/tests/net_quic.rs` that
  asserts handshake payload echoing and DER persistence across restarts.
- First-party UDP + TLS handshake for the in-house provider lives under `crates/transport/src/inhouse/` with message encoding, certificate generation, retransmission/backoff scheduling, TTL-governed handshake tables, and JSON-backed advertisement storage that now persists Ed25519 verifying keys; end-to-end tests in `crates/transport/tests/inhouse.rs` exercise handshake success, certificate mismatches, rotation persistence, and the retry flow without Quinn/s2n dependencies.
- Latest transport coverage extends those suites with handshake latency/reuse assertions and Quinn↔in-house mismatch guards. `crates/transport/tests/inhouse.rs` now records callback firing, session reuse, and failure metadata, while `crates/transport/tests/provider_mismatch.rs` validates mixed-provider registries when both features are compiled.
- Default transport configuration now promotes the in-house provider whenever it is compiled (`crates/transport/src/lib.rs`, `node/Cargo.toml`), ensuring new nodes boot on the first-party adapter while keeping Quinn/s2n available for parity comparisons.
- Overlay abstraction via `crates/p2p_overlay` with in-house and stub backends, configuration toggles, CLI overrides, JSON-backed persistence, integration tests exercising the in-house backend, telemetry gauges (`overlay_backend_active`, `overlay_peer_total`, persisted counts) exposed through `node/src/telemetry.rs`, `cli/src/net.rs`, and `node/src/rpc/peer.rs`, and base58-check peer IDs wired through CLI/RPC/gateway diagnostics, including the latest fanout set surfaced in `net gossip_status`.
- Provider metadata and certificate validation now flow through `p2p::handshake`, which consumes the registry capability enums, persists provider IDs for CLI/RPC output, and loads retry/certificate policies from `config/quic.toml`.
- Peer certificate persistence and config reloads rely on the in-house runtime
  file watcher (`crates/runtime/src/fs/watch.rs`) backed by the refreshed
  `sys::inotify`/`sys::kqueue` wrappers and the first-party `sys::reactor`
  registration path, removing the last `mio`/`nix`/`libc` bridge while keeping
  recursive directory coverage first-party. Tests remain in
  `node/tests/net_quic_certs.rs` and `node/tests/config_watch.rs`.
- `net.quic_stats` RPC and `blockctl net quic stats` expose cached latency,
  retransmit, and endpoint reuse data with per-peer failure metrics for operators.
- LRU-backed duplicate suppression, adaptive fanout, and shard-aware persistence documented in `docs/gossip.md` and implemented in `node/src/gossip/relay.rs` with configurable TTL/fanout stored in `config/gossip.toml`.
  - `net gossip-status` CLI / `net.gossip_status` RPC expose live TTL, cache, fanout, partition tags, and persisted shard peer sets for operators.
  - Peer identifier fuzzing prevents malformed IDs from crashing DHT routing (`net/fuzz/peer_id.rs`).
  - Manual DHT recovery runbook (`docs/networking.md#dht-recovery`).
  - Peer database and chunk cache persist across restarts with configurable paths (`node/src/net/peer.rs` via `TB_PEER_DB_PATH` and `TB_CHUNK_DB_PATH`); `TB_PEER_SEED` fixes shuffle order for reproducible bootstraps.
  - Telemetry handle acquisition now logs failures instead of panicking via the
    shared `telemetry_handle`/`with_metric_handle` helpers, keeping peer event
    processing alive while surfacing structured warnings for operators.
    - Peer metrics persistence now tolerates clock skew and store init failures:
      `node/src/net/peer_metrics_store.rs` falls back to empty snapshots when
      sled can’t open the backing tree and emits structured warnings when the
      system clock moves backwards instead of unwrapping poisoned state.
  - WAN chaos scenarios run through the first-party `ChaosHarness`
    (`sim/src/chaos.rs`) and `chaos_lab` binary, emitting signed readiness
    attestations for overlay/storage/compute. The metrics aggregator verifies
    `/chaos/attest` payloads, publishes `/chaos/status`, and exports
    `chaos_readiness{module,scenario}`, `chaos_site_readiness{module,site}`, and
    `chaos_sla_breach_total`; CI gates releases via `just chaos-suite` and
    `cargo xtask chaos`.
    - The `metrics-aggregator` integration suite now posts the `chaos_lab`
      artefacts through `/chaos/attest` (`chaos_lab_attestations_flow_through_status`)
      and asserts `/chaos/status`, gauge updates, and signer digests, closing the
      loop between the simulation harness and the aggregator without third-party
      dependencies. Grafana’s auto-generated dashboards (`monitoring/src/dashboard.rs`
      → `monitoring/grafana/*.json`) gained a dedicated **Chaos** row charting
      module and site readiness alongside five-minute SLA breach deltas for
      operators.
    - `chaos_lab` now scripts provider failover drills via
      `provider_failover_reports`, emits `chaos_provider_failover.json`, and fails
      when any provider outage does not surface a diff entry or readiness drop.
      `cargo xtask chaos` ingests the failover artefact, enforces overlay
      regression gating (readiness drops, removed sites), and blocks release
      tagging when mixed-provider overlays regress relative to the persisted
      baseline. Overlay soak automation can diff `/chaos/status` snapshots and
      consume the failover summaries without leaving first-party tooling.
    - `sim/did.rs` now assembles DID documents through the first-party JSON helpers
      (no `serde` derive) so the DID simulator runs cleanly during full workspace
      test sweeps.
    - The aggregator rejects forged signatures before mutating readiness gauges
      (`chaos_attestation_rejects_invalid_signature`) and now fails closed on
      malformed module labels or truncated signer arrays, while
      `node/src/gossip/relay.rs` degrades to an in-memory shard cache when
      temporary directories cannot be created, eliminating the last panic in the
      gossip relay path. Gossip node startup returns `io::Result<JoinHandle>` and
      logs key persistence failures so chaos rehearsals continue even when bind
      or fsync operations fail.
  - ASN-aware A* routing oracle (`node/src/net/a_star.rs`) chooses k cheapest paths per shard and feeds compute-placement SLAs.
  - SIMD Xor8 rate-limit filter with AVX2/NEON dispatch (`node/src/web/rate_limit.rs`, `docs/benchmarks.md`) handles 1 M rps bursts.
  - Jittered JSON‑RPC client with exponential backoff (`node/src/rpc/client.rs`) prevents thundering-herd reconnect storms.
  - Gateway DNS publishing and policy retrieval logged in `docs/gateway_dns.md` and implemented in `node/src/gateway/dns.rs`.
- Per-peer rate-limit telemetry and reputation tracking via `net.peer_stats` RPC and `net stats` CLI, capped by `max_peer_metrics`, with dashboards ingesting `GOSSIP_PEER_FAILURE_TOTAL` and `GOSSIP_LATENCY_BUCKETS`.
    - Peer metrics sled snapshots now encode/decode via `node/src/net/peer_metrics_binary.rs`, keeping persistence on the binary cursor helpers while JSON exports continue to leverage facade derives; compatibility tests guard the legacy layout.
     - Partition watch detects split-brain conditions and stamps gossip with markers (`node/src/net/partition_watch.rs`, `node/src/gossip/relay.rs`).
     - Cluster-wide metrics pushed to the `metrics-aggregator` crate for fleet visibility.
    - Shard-aware peer maps and gossip routing limit block broadcasts to interested shards (`node/src/gossip/relay.rs`).
    - Uptime-based fee rebates tracked in `node/src/net/uptime.rs` with `peer.rebate_status` RPC (`docs/fee_rebates.md`).

**Gaps**
- Bootstrap peer churn analysis missing.
    - Overlay soak tests need long-lived fault injection, and the dependency registry now focuses on automating storage migration drills plus the upcoming dependency fault simulation harness to certify fallbacks.

## 3. Governance & Subsidy Economy — 96.4 %

**Evidence**
- Subsidy multiplier proposals surfaced via `node/src/rpc/governance.rs` and web UI (`tools/gov-ui`).
- Shared `governance` crate re-exports bicameral voting, first-party sled-backed `GovStore`, proposal DAG validation, Kalman retune helpers, and release workflows for CLI/SDK consumers (`governance/src/lib.rs` and examples).
- Push notifications on subsidy balance changes (`wallet` tooling).
- Explorer indexes settlement receipts with query endpoints (`explorer/src/lib.rs`).
- Risk-sensitive Kalman–LQG governor with variance-aware smoothing (`node/src/governance/kalman.rs`, `node/src/governance/variance.rs`).
- Laplace-noised multiplier releases and miner-count logistic hysteresis (`node/src/governance/params.rs`, `pow/src/reward.rs`).
- Emergency kill switch `kill_switch_subsidy_reduction` with telemetry counters (`node/src/governance/params.rs`, `docs/monitoring.md`).
- Subsidy accounting is unified in the CT ledger with migration documented in `docs/system_changes.md`.
- Proof-rebate tracker now persists per-relayer receipts via the first-party binary cursor (`node/src/light_client/proof_tracker.rs`, `node/src/util/binary_struct.rs`) with governance rate clamps and coinbase integration (`node/src/blockchain/process.rs`, `docs/light_client_incentives.md`).
- Multi-signature release approvals persist signer sets and thresholds (`node/src/governance/release.rs`), gated fetch/install flows (`node/src/update.rs`, `cli/src/gov.rs`), and explorer/CLI timelines (`explorer/src/release_view.rs`, `contract explorer release-history`).
- Telemetry counters `release_quorum_fail_total` and `release_installs_total` expose quorum health and rollout adoption for dashboards.
- Fee-floor window and percentile parameters (`node/src/governance/params.rs`) stream through `GovStore` history with rollback support (`node/src/governance/store.rs`), governance CLI updates (`cli/src/gov.rs`), explorer timelines (`explorer/src/lib.rs`), and regression coverage (`governance/tests/mempool_params.rs`).
- DID revocations share the same `GovStore` history and prevent further anchors until governance clears the entry; the history is available to explorer and wallet tooling so revocation state can be surfaced alongside DID records (`node/src/governance/store.rs`, `node/src/identity/did.rs`, `docs/identity.md`).
- Simulations `sim/release_signers.rs` and `sim/lagging_release.rs` model signer churn and staggered downloads to validate quorum durability and rollback safeguards before production deployment.
- One‑dial multiplier formula retunes β/γ/κ/λ per epoch using realised utilisation `U_x`, clamped to ±15 % and doubled when `U_x` → 0; see `docs/economics.md`.
- Demand gauges `industrial_backlog` and `industrial_utilization` feed
    `Block::industrial_subsidies()` and surface via `inflation.params` and
    `compute_market.stats`.
- `pct_ct` tracks CT fee routing; production lanes pin the selector to 100 while `reserve_pending` debits balances before coinbase accumulation (`docs/fees.md`).
- Logistic base reward `R_0(N) = R_max / (1 + e^{ξ (N - N^*)})` with hysteresis `ΔN ≈ √N*` dampens miner churn and is implemented in `pow/src/reward.rs`.
 - Kalman filter weights for difficulty retune configurable via governance parameters (`node/src/governance/params.rs`).

**Gaps**
- Publish explorer timelines for proposal windows and upcoming treasury disbursements emitted by the CLI/governance crate.
- No on‑chain treasury or proposal dependency system.
- Governance rollback simulation incomplete.

## 4. Storage & Free‑Read Hosting — 93.8 %

**Evidence**
- Read acknowledgement batching and audit flow documented in `docs/read_receipts.md` and `docs/storage_pipeline.md`.
- Disk‑full metrics and recovery tests (`node/tests/storage_disk_full.rs`).
- Gateway HTTP parsing fuzz harness (`gateway/fuzz`).
- In-house LT fountain overlay for BLE repair (`node/src/storage/repair.rs`, `docs/storage/repair.md`, `node/tests/fountain_repair.rs`).
- Thread-safe `ReadStats` telemetry and analytics RPC (`node/src/telemetry.rs`, `node/tests/analytics.rs`).
- WAL-backed `SimpleDb` design in `docs/simple_db.md` underpins DNS cache, chunk gossip, and DEX storage.
- Unified `storage_engine` crate wraps RocksDB, the first-party sled crate, and in-memory engines with shared traits, concurrency-safe batches, crash-tested temp dirs, and configuration-driven overrides (`crates/storage_engine`, `node/src/simple_db/mod.rs`).
- `crates/coding` fronts encryption, erasure, fountain, and compression primitives; XOR parity and RLE fallback compressors respect `config/storage.toml` rollout gates, emit coder/compressor labels on storage latency and failure metrics, log `algorithm_limited` repair skips, and feed the `bench-harness compare-coders` mode for performance baselining (`crates/coding/src`, `node/src/storage/settings.rs`, `tools/bench-harness/src/main.rs`).
- Base64 snapshots stage through `NamedTempFile::persist` plus `sync_all`, with legacy dumps removed only after durable rename (`node/src/simple_db/memory.rs`, `node/tests/simple_db/memory_tests.rs`).
- Rent escrow metrics (`rent_escrow_locked_ct_total`, etc.) exposed in `docs/monitoring.md` with alert thresholds.
- Sled-backed rent escrow, manifests, provider profiles, and repair failure
  records persist via first-party cursor helpers with regression tests covering
  large manifests, redundancy variants, and legacy payloads that omit modern
  optional fields, plus a randomized property suite and sparse-metadata repair
  integration test that keep parity with the legacy binary codec (`node/src/storage/{fs.rs,manifest_binary.rs,pipeline/binary.rs,repair.rs}`, `storage/tests/repair.rs`).
- Metrics aggregator ingestion now runs on the in-house `httpd` router; outbound log correlation calls continue to use the shared `httpd::HttpClient` (`metrics-aggregator/src/lib.rs`). Snapshot exports now rely on the first-party SigV4 uploader layered on `httpd::HttpClient`, removing the AWS SDK while keeping S3 compatibility against the in-house object store. Runtime-backed ingestion and retention rework remain outstanding.
- Metrics aggregator leader election now operates on the first-party `InhouseEngine` lease table, eliminating the `etcd-client`/tonic/Tokio stack and keeping coordination inside the runtime facade (`metrics-aggregator/src/leader.rs`, `docs/monitoring.md`).
- Mobile gateway cache persists ChaCha20-Poly1305–encrypted responses and queued transactions to the first-party sled store with TTL sweeping, eviction guardrails, telemetry counters, CLI `mobile-cache status|flush` commands, RPC inspection endpoints, and invalidation hooks (`node/src/gateway/mobile_cache.rs`, `node/src/rpc/gateway.rs`, `cli/src/gateway.rs`, `docs/mobile_gateway.md`). A min-heap of expirations drives sweep cadence, persistence snapshots reconstruct queues on restart, encryption keys derive from `TB_MOBILE_CACHE_KEY_HEX`/`TB_NODE_KEY_HEX`, and status responses expose per-entry age/expiry plus queue bytes so operators can tune TTL windows and capacity.
- Reputation-weighted Lagrange allocation and proof-of-retrievability challenges secure storage contracts (`node/src/gateway/storage_alloc.rs`, `storage/src/contract.rs`).

**Gaps**
- Incentive‑backed DHT storage marketplace still conceptual.
- Offline escrow reconciliation absent.

## 5. Smart‑Contract VM & UTXO/PoW — 87.5 %

**Evidence**
- Persistent `ContractStore` with CLI deploy/call flows (`state/src/contracts`, `cli/src/main.rs`).
- ABI generation from opcode enum (`node/src/vm/opcodes.rs`).
- State survives restarts (`node/tests/vm.rs::state_persists_across_restarts`).
- Planned dynamic gas fee market (`node/src/fees.rs` roadmap) anchors eventual EIP-1559 adaptation.
- Deterministic WASM runtime with fuel-based metering and ABI helpers (`node/src/vm/wasm/mod.rs`, `node/src/vm/wasm/gas.rs`).
- Interactive debugger and trace export (`node/src/vm/debugger.rs`, `docs/vm_debugging.md`).
- VM trace WebSocket streaming now rides the in-house runtime sockets (`node/src/rpc/vm_trace.rs`, `crates/runtime/src/net.rs`), keeping debugger tooling aligned with the dependency-sovereignty goals.

**Gaps**
- Instruction set remains minimal; no formal VM spec or audits.
- Developer SDK and security tooling pending.

## 6. Compute Marketplace & CBM — 95.8 %

**Evidence**
- Deterministic GPU/CPU hash runners (`node/src/compute_market/workloads`).
- Compute marketplace RPC endpoints still run through the bespoke parser backed by `runtime::net::TcpListener` in `node/src/rpc/mod.rs`; the `crates/httpd` router remains unused on the server side, so the dependency risk persists until that migration lands (`node/tests/compute_market_rpc_errors.rs`).
- `compute.job_cancel` RPC releases resources and refunds bonds (`node/src/rpc/compute_market.rs`).
- Capability-aware scheduler matches CPU/GPU workloads, weights offers by provider reputation, and handles cancellations (`node/src/compute_market/scheduler.rs`).
- Price board persistence with metrics (`docs/compute_market.md`).
- Lane-aware matching enforces per-`FeeLane` queues, fairness windows, and starvation timers, throttles via `TB_COMPUTE_MATCH_BATCH`, records `MATCH_LOOP_LATENCY_SECONDS{lane}` histograms, persists receipts with lane tags for replay safety, and surfaces queue depths/capacity guardrails through RPC/CLI (`node/src/compute_market/matcher.rs`, `node/tests/compute_matcher.rs`, `node/src/rpc/compute_market.rs`, `cli/src/compute.rs`). The matcher rotates lanes until a batch quota or fairness deadline triggers, rejects staged seeds that exceed capacity, emits structured starvation warnings with job IDs/ages, and annotates `compute_market.stats` with per-lane wait durations for operators.
- Settlement persists CT balances, audit logs, activation metadata, SLA queues, and Merkle roots in a RocksDB-backed store with RPC/CLI/explorer surfacing (`node/src/compute_market/settlement.rs`, `node/tests/compute_settlement.rs`, `docs/compute_market.md`, `docs/settlement_audit.md`, `explorer/src/compute_view.rs`). The ledger emits telemetry (`SETTLE_APPLIED_TOTAL`, `SETTLE_FAILED_TOTAL{reason}`, `SETTLE_MODE_CHANGE_TOTAL{state}`, `SLASHING_BURN_CT_TOTAL`, `COMPUTE_SLA_VIOLATIONS_TOTAL{provider}`, `COMPUTE_SLA_PENDING_TOTAL`, `COMPUTE_SLA_NEXT_DEADLINE_TS`, `COMPUTE_SLA_AUTOMATED_SLASH_TOTAL`) and exposes `compute_market.provider_balances`, `compute_market.audit`, and `compute_market.recent_roots` RPCs for automated reconciliation.
- `Settlement::shutdown` persists any pending ledger deltas and flushes RocksDB handles before teardown so test harnesses (and unplanned exits) leave behind consistent CT balances and Merkle roots for replay.
- Admission enforces dynamic fee floors with per-sender slot caps, eviction audit trails, explorer charts, and `mempool.stats` exposure (`node/src/mempool/admission.rs`, `node/src/mempool/scoring.rs`, `docs/mempool_qos.md`, `node/tests/mempool_eviction.rs`). Governance parameters for the floor window and percentile stream through telemetry (`fee_floor_window_changed_total`, `fee_floor_warning_total`, `fee_floor_override_total`) and wallet guidance.
- `FeeFloor::new(size, percentile)` now requires explicit percentile inputs in tests and CLI paths, aligning mempool QoS regressions with governance-configured sampling windows (`node/src/mempool/scoring.rs`, `node/tests/mempool_qos.rs`).
- Economic simulator outputs KPIs to CSV (`sim/src`).
- Durable courier receipts with exponential backoff documented in `docs/compute_market_courier.md` and implemented in `node/src/compute_market/courier.rs`.
- Groth16/Plonk SNARK verification for compute receipts (`node/src/compute_market/snark.rs`).
- Policy pins `fee_pct_ct` to CT-only payouts for production lanes while retaining selector compatibility in tests (`node/src/compute_market/mod.rs`, `docs/compute_market.md`).

**Gaps**
- SLA telemetry now powers automated slashing dashboards; remaining work is to wire Grafana alerting and aggregator exports to page when `COMPUTE_SLA_PENDING_TOTAL` grows without matching automated slashes.

## 7. Trust Lines & DEX — 89.6 %

**Evidence**
- Persistent order books via `node/src/dex/storage.rs` and restart tests (`node/tests/dex_persistence.rs`).
- First-party sled persistence via `node/src/dex/{storage.rs,storage_binary.rs}` encodes order books, trade logs, pools, and escrow state with cursor helpers and randomized regression suites that match the legacy bytes (`order_book_matches_legacy`, `trade_log_matches_legacy`, `escrow_state_matches_legacy`, `pool_matches_legacy`).
- Cost‑based multi‑hop routing with fallback paths (`node/src/dex/trust_lines.rs`).
- On-ledger escrow with partial-payment proofs (`dex/src/escrow.rs`, `node/tests/dex.rs`, `dex/tests/escrow.rs`).
- Trade logging and routing semantics documented in `docs/dex.md`.
- CLI escrow flows and Merkle proof verification exposed via `dex escrow status`/
  `dex escrow release` commands and `dex.escrow_proof` RPC. Telemetry gauges
  `dex_escrow_locked`, `dex_escrow_pending`, and `dex_escrow_total` monitor
  utilisation; `dex_escrow_total` aggregates locked funds across all escrows.
- Constant-product AMM pools and liquidity mining incentives (`dex/src/amm.rs`, `docs/dex_amm.md`).
- Deterministic liquidity router batches escrow releases, bridge withdrawals, and
  trust-line rebalances with MEV-resistant ordering (`node/src/liquidity/router.rs`,
  `docs/dex.md`). Governance steers batch size, fairness jitter, hop limits, and
  rebalance thresholds while the router enforces trust-path availability via
  `TrustLedger::settle_path`.

**Gaps**
- Escrow for cross‑chain DEX routes absent.

## 8. Wallets, Light Clients & KYC — 96.6 %

**Evidence**
- CLI + hardware wallet support (`crates/wallet`).
- Remote signer workflows (`crates/wallet/src/remote_signer.rs`, `docs/wallets.md`).
- Remote signer HTTP calls now rely on the blocking wrapper in `crates/httpd`, eliminating external clients while keeping retry/backoff semantics intact (`crates/wallet/src/remote_signer.rs`, `crates/httpd/src/blocking.rs`).
- Wallet remote signer TLS now uses the first-party `httpd::TlsConnector`, JSON trust anchors, and certificate tooling, removing the `native-tls` shim while preserving client auth coverage in `crates/wallet/tests/remote_signer.rs` and `crates/httpd/src/tls_client.rs`.
- CLI RPC flows, node HTTP helpers, and the metrics aggregator now consume the
  `httpd::TlsConnector` via shared helpers so trust anchors and client
  identities come from environment prefixes (`cli/src/http_client.rs`,
  `node/src/http_client.rs`, `metrics-aggregator/src/lib.rs`,
  `metrics-aggregator/src/object_store.rs`).
- Mobile light client with push notification hooks (`examples/mobile`, `docs/mobile_light_client.md`).
- Light-client synchronization and header verification documented in `docs/light_client.md`.
- Device status probes integrate Android/iOS power and connectivity hints, cache asynchronous readings with graceful degradation, emit `the_block_light_client_device_status{field,freshness}` telemetry, persist overrides in `~/.the_block/light_client.toml`, surface CLI/RPC gating messages, and embed annotated snapshots in compressed log uploads (`crates/light-client`, `cli/src/light_client.rs`, `docs/light_client.md`, `docs/mobile_light_client.md`). The Android and iOS implementations now depend solely on first-party helpers—`sys::device::{battery,network}` reads `/sys` and `/proc` sensors while the iOS probe issues Objective-C/CoreFoundation calls through in-house FFI—dropping the legacy `jni`, `ndk`, and `objc` stacks.
- Real-time state streaming over WebSockets with hybrid (lz77-rle) snapshots (`docs/light_client_stream.md`, `node/src/rpc/state_stream.rs`).
- Optional KYC provider wiring (`docs/kyc.md`).
- Session-key issuance and meta-transaction tooling (`crypto/src/session.rs`, `cli/src/wallet.rs`, `docs/account_abstraction.md`).
- Telemetry `session_key_issued_total`/`session_key_expired_total` and simulator churn knob (`sim/src/lib.rs`).
- Release fetch/install tooling verifies provenance, records timestamps, and exposes explorer/CLI history for operator audits (`node/src/update.rs`, `cli/src/gov.rs`, `explorer/src/release_view.rs`).
- Wallet send flow caches fee-floor lookups, emits localized warnings with auto-bump or `--force` overrides, streams telemetry events back to the node, and exposes JSON mode for automation (`cli/src/wallet.rs`, `docs/mempool_qos.md`).
- Unified crypto suite Ed25519 signature handling (first-party backend) ensures remote signer payloads, CLI staking flows, and explorer attestations all share compatible types while forwarding multisig signer arrays and escrow hash algorithms (`crates/wallet`, `node/src/bin/wallet.rs`, `tests/remote_signer_multisig.rs`).
- Remote signer metrics (`remote_signer_request_total`, `remote_signer_success_total`, `remote_signer_error_total{reason}`) integrate with wallet QoS counters so dashboards highlight signer outages alongside fee-floor overrides (`crates/wallet/src/remote_signer.rs`, `docs/monitoring.md`).
- Light-client rebate history and leaderboards exposed via RPC/CLI/explorer (`node/src/rpc/light.rs`, `cli/src/light_client.rs`, `explorer/src/light_client.rs`, `docs/light_client_incentives.md`).

**Gaps**
- Polish multisig UX (batched signer discovery, richer operator prompts) before tagging the next CLI release.
- Surface multisig signer history in explorer/CLI output for auditability.
- Production‑grade mobile apps not yet shipped.

## 9. Bridges & Cross‑Chain Routing — 93.1 %

**Evidence**
- Per-asset bridge channels with relayer sets, pending withdrawals, and bond ledgers persisted via `SimpleDb` (`node/src/bridge/mod.rs`).
- Governance-controlled incentive parameters (`BridgeIncentiveParameters`) drive minimum bond, reward, and slashing rules, with refreshed integration coverage in `node/tests/bridge.rs` and the new `node/tests/bridge_incentives.rs` exercising honest/faulty relayers end to end.
- Sled-backed duty ledger and per-relayer accounting snapshots expose rewards, penalties, and duty history via `bridge.relayer_accounting`/`bridge.duty_log` and the matching CLI commands (`blockctl bridge accounting`, `blockctl bridge duties`).
- Multi-signature quorum enforcement and governance authorization hooks in `bridge.verify_deposit` and `governance::ensure_release_authorized` remain active, with duty outcomes persisted for audit.
- Challenge windows and slashing logic (`bridge.challenge_withdrawal`, `bridges/src/relayer.rs`) debit collateral according to the configured `failure_slash`/`challenge_slash` and emit telemetry `BRIDGE_CHALLENGES_TOTAL`/`BRIDGE_SLASHES_TOTAL`, while reward claims, settlement submissions, and duty outcomes update `BRIDGE_REWARD_CLAIMS_TOTAL`, `BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL`, `BRIDGE_SETTLEMENT_RESULTS_TOTAL{result,reason}`, and `BRIDGE_DISPUTE_OUTCOMES_TOTAL{kind,outcome}`.
- Partition markers propagate through deposit events and withdrawal routing so relayers avoid isolated shards (`node/src/net/partition_watch.rs`, `docs/bridges.md`).
- CLI/RPC surfaces for quorum composition, pending withdrawals, history, slash logs, accounting, and duty logs (`cli/src/bridge.rs`, `node/src/rpc/bridge.rs`).
- Liquidity router integration sequences matured withdrawals alongside DEX
  escrows and trust-line rebalances, with bridge finalisers enforcing the router’s
  order for MEV-resistant FX (`node/src/liquidity/router.rs`, `docs/bridges.md`).
- Bridge alerting now includes per-label skew rules (`BridgeCounterDeltaLabelSkew`, `BridgeCounterRateLabelSkew`) with the first-party `bridge-alert-validator` binary exercising `monitoring/alert.rules.yml` in CI so asset-specific anomalies page operations without third-party tooling. The shared `monitoring/src/alert_validator.rs` module now replays canned datasets for bridge, chain-health, dependency-registry, and treasury groups in one pass, and the bridge fixtures cover recovery tails, partial windows, dispute outcomes, and quorum-failure approvals. Labelled spikes feed the persisted remediation engine that serves `/remediation/bridge` alongside the `bridge_remediation_action_total{action,playbook}` counter and the liquidity telemetry (`bridge_liquidity_locked_total`, `bridge_liquidity_unlocked_total`, `bridge_liquidity_minted_total`, `bridge_liquidity_burned_total`).
- Remediation actions now dispatch automatically through the aggregator’s first-party hooks. Environment-configurable `TB_REMEDIATION_*_URLS`/`*_DIRS` deliver JSON payloads to paging/governance services or local spools, the operator playbook documents the liquidity response flow end to end, and `bridge_remediation_dispatch_total{action,playbook,target,status}` plus the `/remediation/bridge/dispatches` endpoint record success, skip, and failure outcomes for every target. Dispatch payloads now embed annotations, dashboard-panel hints, and a response sequence so paging/governance automation can execute the runbook without bespoke glue.
- Downstream acknowledgement telemetry is first party: the aggregator increments `bridge_remediation_dispatch_ack_total{action,playbook,target,state}`, records acknowledgement/closure timestamps and notes on each remediation action, and the Grafana row charts acknowledgement deltas next to dispatch totals so operators can prove paging/governance hooks closed the loop.
- Automated follow-ups now live entirely inside the remediation engine. Pending actions track `dispatch_attempts`, `auto_retry_count`, retry timestamps, and cumulative follow-up notes so the aggregator can queue deterministic auto-retry payloads before escalating to governance playbooks when policy thresholds expire. The acknowledgement parser tolerates plain-text hook responses, promoting strings like `"ack pager"` or `"closed: resolved"` into structured records, and new alerts (`BridgeRemediationAckPending`, `BridgeRemediationClosureMissing`) page operators when acknowledgements stall or closures never arrive.
- Per-playbook acknowledgement policy is driven entirely by environment: `TB_REMEDIATION_ACK_RETRY_SECS`, `_ESCALATE_SECS`, and `_MAX_RETRIES` seed defaults, while suffix overrides such as `..._GOVERNANCE_ESCALATION` tighten or relax retry/escalation windows for sensitive hooks. The aggregator records completion latency in the new `bridge_remediation_ack_latency_seconds{playbook,state}` histogram so Grafana and the HTML snapshot chart p50/p95 acknowledgement times alongside the dispatch counters, and the first-party `contract remediation bridge` command streams the same action/dispatch history for on-call operators without relying on external tooling.

**Gaps**
- Treasury sweep automation, offline settlement proof sampling, and richer relayer incentive analytics remain.
## 10. Monitoring, Debugging & Profiling — 97.2 %

**Evidence**
  - Runtime telemetry exporter with extensive counters (`node/src/telemetry.rs`).
  - Service badge tracker exports uptime metrics and RPC status (`node/src/service_badge.rs`, `node/tests/service_badge.rs`). See `docs/service_badge.md`.
  - Monitoring stack via `make monitor` and docs in `docs/monitoring/README.md`.
    - Cluster metrics aggregation with disk-backed retention (`metrics-aggregator` crate).
    - Aggregator ingestion now depends solely on the in-house `httpd` server; runtime-backed archive streaming is pending. Outbound correlations continue to share the node’s HTTP client (`metrics-aggregator/src/lib.rs`).
    - Metrics-to-logs correlation links runtime telemetry anomalies to targeted log dumps and exposes `log_correlation_fail_total` for missed lookups (`metrics-aggregator/src/lib.rs`, `node/src/rpc/logs.rs`, `cli/src/logs.rs`).
    - VM trace counters and partition dashboards (`node/src/telemetry.rs`, `monitoring/templates/partition.json`).
    - Settlement audit CI job (`.github/workflows/ci.yml`).
    - Fee-floor policy changes and wallet overrides surface via `fee_floor_window_changed_total`, `fee_floor_warning_total`, and `fee_floor_override_total`, while DID anchors increment `did_anchor_total` for explorer dashboards (`node/src/telemetry.rs`, `monitoring/metrics.json`, `docs/mempool_qos.md`, `docs/identity.md`).
    - Per-lane compute matcher counters (`matches_total{lane}`), latency histograms (`match_loop_latency_seconds{lane}`), starvation warnings, and mobile cache metrics (`mobile_cache_hit_total`, `mobile_cache_stale_total`, `mobile_cache_entry_bytes`, `mobile_cache_queue_bytes`, `mobile_tx_queue_depth`) feed dashboards alongside the `the_block_light_client_device_status{field,freshness}` gauge for background sync diagnostics (`node/src/telemetry.rs`, `docs/telemetry.md`, `docs/mobile_gateway.md`, `docs/light_client.md`).
    - Storage ingest and repair metrics carry `erasure`/`compression` labels so fallback rollouts can be tracked in Grafana, and repair skips log `algorithm_limited` contexts for incident reviews (`node/src/telemetry.rs`, `docs/monitoring.md`, `docs/storage_erasure.md`).
- Wrapper telemetry exports runtime/transport/overlay/storage/coding/codec/crypto metadata via `runtime_backend_info`, `transport_provider_connect_total{provider}`, `codec_serialize_fail_total{profile}`, and `crypto_suite_signature_fail_total{backend}`. The `metrics-aggregator` exposes a `/wrappers` endpoint for fleet summaries, Grafana dashboards render backend selections/failure rates, and `contract-cli system dependencies` fetches on-demand snapshots for operators (`node/src/telemetry.rs`, `metrics-aggregator/src/lib.rs`, `monitoring/metrics.json`, `monitoring/grafana/*.json`, `cli/src/system.rs`).
- Bulk peer exports encrypt with the in-house envelope (`crypto_suite::encryption::envelope`) so operators can download archives with either X25519 recipients (`application/tb-envelope`) or shared passwords (`application/tb-password-envelope`) without touching `age` or OpenSSL (`metrics-aggregator/src/lib.rs`, `docs/monitoring.md`, `node/src/bin/net.rs`).
    - Incremental log indexer resumes from offsets, rotates encryption keys, streams over WebSocket, and exposes REST filters (`tools/log_indexer.rs`, `docs/logging.md`).
- Bridge alert rules now include label-specific skew detection and the CI-run `bridge-alert-validator` binary verifies expressions against canned datasets, keeping alert coverage first party without promtool (`monitoring/src/alert_validator.rs`, `.github/workflows/ci.yml`). The binary now validates the bridge, chain-health, dependency-registry, and treasury alert groups in a single invocation.
- The bridge dataset now includes recovery curves and partial-window fixtures so the `BridgeCounter*Skew` heuristics stay quiet during cooldowns. Remediation actions fan out to first-party HTTP or spool hooks (`TB_REMEDIATION_*_URLS`/`*_DIRS`), and the operator incident playbook documents the liquidity response flow alongside dashboard annotations.
- Bridge dashboards now chart acknowledgement latency directly via the new `bridge_remediation_ack_latency_seconds{playbook,state}` histogram, overlay the policy gauge `bridge_remediation_ack_target_seconds{playbook,policy}`, and persist samples across restarts so p50/p95 latency and policy targets remain visible next to the dispatch counters without leaving the first-party stack.

**Gaps**
- Continue building VM anomaly heuristics and long-horizon performance soak dashboards.

## 11. Identity & Explorer — 83.4 %

**Evidence**
- DID registry persists anchors with replay protection, governance revocation checks, and optional provenance attestations (`node/src/identity/did.rs`, `state/src/did.rs`).
- Light-client commands anchor and resolve DIDs with remote signer support, sign-only payload export, and JSON output for automation (`cli/src/light_client.rs`, `examples/did.json`).
- Explorer ingests DID updates into `did_records`, serves `/dids`, `/identity/dids/:address`, and anchor-rate metrics for dashboards (`explorer/src/did_view.rs`, `explorer/src/main.rs`).
- Explorer caches DID lookups in-memory to avoid redundant RocksDB reads and drives anchor-rate dashboards from `/dids/metrics/anchor_rate` (`explorer/src/did_view.rs`, `explorer/src/main.rs`).
- Governance history captures fee-floor and DID revocations for auditing alongside wallet telemetry (`node/src/governance/store.rs`, `docs/identity.md`).
- Handle registry normalization now runs on the `foundation_unicode` facade with Latin-1/Greek transliteration, emits
  `identity_handle_normalization_total{accuracy}`, and the CLI mirrors these results via `contract identity register|normalize`
  (`crates/foundation_unicode`, `node/src/identity/handle_registry.rs`, `cli/src/identity.rs`).

**Gaps**
- Revocation alerting and recovery runbooks need explorer/CLI integration.
- Mobile wallet identity UX and bulk export tooling remain outstanding.

## 12. Economic Simulation & Formal Verification — 43.0 %

**Evidence**
- Simulation scenarios for inflation/demand/backlog (`sim/src`).
- F* scaffolding for consensus proofs (`formal/` installers and docs).
- Scenario library exports KPIs to CSV.

**Gaps**
- Formal proofs beyond scaffolding missing.
- Scenario coverage still thin.

## 13. Mobile UX & Contribution Metrics — 73.2 %

**Evidence**
- Background sync respecting battery/network constraints with platform-specific probes, async caching, CLI/RPC gating messages, and persisted overrides (`docs/light_client.md`, `docs/mobile_light_client.md`, `cli/src/light_client.rs`). Device snapshots capture freshness (`fresh|cached|fallback`) labels, stream to `the_block_light_client_device_status`, embed into compressed log uploads, and expose CLI toggles for charging/Wi‑Fi overrides stored in `~/.the_block/light_client.toml`.
- Contribution metrics and optional KYC in mobile example (`examples/mobile`).
- Push notifications for subsidy events (wallet tooling) plus encrypted mobile cache persistence with TTL hygiene, size guardrails, and CLI flush hooks for reliable offline recovery (`node/src/gateway/mobile_cache.rs`, `docs/mobile_gateway.md`).

**Gaps**
- Broad hardware testing and production app distribution outstanding.
- Remote signer support for mobile not yet built.

---

*Last updated: 2025‑10‑10*
