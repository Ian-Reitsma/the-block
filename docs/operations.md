# Operations Runbooks: Fast-Mainnet

**Purpose**: Step-by-step troubleshooting guides for operational issues  
**Audience**: Site Reliability Engineers, Launch Operations Team  
**Related**: `docs/OBSERVABILITY_MAP.md` (mapping questions to metrics)

---

## Dependency governance

- **Zero third-party rule**: registry/git dependencies are forbidden. Keep `FIRST_PARTY_ONLY=1`, ensure `dependency_guard` passes in build scripts, and reject deployments/PRs that add non-workspace crates. When vendoring is adjusted, refresh `config/dependency_policies.toml`, `provenance.json`, and `checksums.txt`, and surface the change in CI notes so ops can audit supply-chain drift before tagging.

## Telemetry Wiring

### BlockTorch receipt telemetry & plan enforcement

- **Objective**: `docs/blocktorch/BLOCKCHAIN_INTEGRATION_STRATEGY.md`, `docs/blocktorch_compute_integration_plan.md`, `docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md#part-xii`, and this runbook form the triad for wiring BlockTorch receipts into observability. Treat the plan as your authoritative “what to do first” list; every telemetry change or governor gate must cite it in the PR summary and the runbook entry.
- **Hard requirements**:
  1. Add new metrics from the plan (`receipt_drain_depth`, `proof_verification_latency`, `kernel_variant.digest`, `benchmark_commit`, `orchard_alloc_free_delta`, `snark_prover_latency_seconds`) inside `node/src/telemetry.rs` and `node/src/telemetry/receipts.rs`.
  2. Guard them behind the `telemetry` cargo feature and sink the allocator logs from `blocktorch/metal/runtime/Allocator.h` (parse `/tmp/orchard_tensor_profile.log` lines into labelled histograms with `job_id` + `device`).
  3. Export these metrics via `metrics-aggregator/telemetry.yaml` so `/wrappers` includes the new series, update `monitoring/tests/snapshots/wrappers.json`, and document the refreshed hash in your PR description.
  4. This change wires the basic gauges/histograms (`receipt_drain_depth`, `proof_verification_latency_ms`, `sla_breach_depth`, `orchard_alloc_free_delta`) into `node/src/telemetry/receipts.rs` and refreshes the `/wrappers` hash (`e1c21b717bd804e816ee7ab9f6876c0832f9813d9d982444cfa13c1186381756`); the remaining plan steps (kernel metadata, CLI/governor outputs, dashboards) still need follow-up wiring.
  4. Refresh `monitoring/` dashboards (`npm ci --prefix monitoring && make monitor`) so panels show proof latency per lane + lane slashing, allocator deltas, kernel digests, and benchmark commits. Capture the new JSON + screenshot references for reviewers.
  5. Log the telemetry exposure here (fields + panel IDs + `/wrappers` hash) and link back to `docs/blocktorch_compute_integration_plan.md` for future reference.

- **Begin wiring instructions** (1% dev action list):
  1. Extend `node/src/telemetry.rs` to emit the listed metrics with receipt metadata, ensure each metric has stable labels for aggregator joining, and add unit tests verifying the metrics decode the `kernel_variant.digest`.
  2. Update `metrics-aggregator/telemetry.yaml` to declare the new gauges/histograms, rerun the wrappers snapshot helper (`WRITE_WRAPPERS_SNAPSHOT=1 cargo test -p metrics-aggregator --test wrappers`), and commit the updated snapshot plus the new hash.
  3. Rebuild Grafana dashboards, include the kernel hash + benchmark panels in `monitoring/` JSON, and refresh `monitoring/tests/snapshots/wrappers.json`. Document the new screenshot names and `make monitor` logs in the PR notes.
  4. Add CLI/governor visibility by extending `cli/src/compute.rs` and `cli/src/governor.rs` to print the kernel hash, benchmark commit, proof latency histogram, and aggregator `/wrappers` hash in their status outputs. Match the format described in the plan (with deterministic field order).
	  4.a. Operators can seed or override the metadata emitted by the node using `TB_BLOCKTORCH_KERNEL_VARIANT_DIGEST` and `TB_BLOCKTORCH_BENCHMARK_COMMIT`. Those knobs back the CLI/governor timeline when the BlockTorch artifacts are not emitted automatically, and the aggregated trace hash surfaces as `blocktorch_aggregator_trace` so you can validate the telemetry bucket you just inspected.
	  4.b. After wiring the timeline, lock its formatting with `cargo test -p contract-cli --features telemetry formatted_blocktorch_timeline`. The test mirrors the CLI/governor order (header → kernel digest → benchmark commit → proof latency → aggregator trace) and fails whenever the helper drifts or the RPC payload drops a field; rerun the test, inspect `node/src/telemetry.rs` + `/wrappers`, and replay the CLI surfaces with `TB_BLOCKTORCH_*` overrides before landing the change.
	  5. Update `docs/system_reference.md`, `docs/overview.md`, and this runbook to mention the updated CLI/governor outputs plus the `TB_BLOCKTORCH_*` knobs (see the plan). Reference the plan when describing how to read the new fields.
  6. After wiring, run the mandated command suite (`just lint`, `just fmt`, `just test-fast`, `just test-full`, `cargo test -p the_block --test replay`, `cargo test -p the_block --test settlement_audit --release`, `scripts/fuzz_coverage.sh`) plus `npm ci --prefix monitoring && make monitor`. Attach logs and the `metrics-aggregator` wrapper hash to the PR, and mention the runbook cross-checks (kernel hash diff, aggregator hash, CLI status sample).

- **New gate**: The BlockTorch governor now manages `proof_verification_budget_ms` (default 100 ms). Settlement inspects the recorded `ProofBundle::latency_ms` values in `SlaRecord::proofs` and flips `SlaOutcome::Completed` into `SlaOutcome::Violated { reason: "proof_latency_budget" }` whenever the max latency breaches the budget, triggering the normal slash/refund/telemetry flow. Reference `docs/system_reference.md#6.3`, cite the BlockTorch timeline (`tb-cli compute stats` / `tb-cli governor status`), and record any manual budget tweaks (or rollback steps) in this runbook.
- **Operator note**: During incidents, query `tb-cli governor status --rpc <endpoint>` (the `telemetry gauges (ppm)` section now includes BlockTorch metrics). Correlate `proof_verification_latency` with the Grafana panel `blocktorch-proofs-latency` and the aggregator trace ID recorded in `/wrappers`.

### Bridge remediation ack latency persistence

- The aggregator now persists only the latest `bridge_remediation_ack_latency_seconds` observation per `(playbook,state)` so restart/replay cycles do not double-count the same acknowledgement. After restarting, reload the snapshot and observe the same `count`/`sum` that were captured before the outage.
- When this wiring changes, rerun `WRITE_WRAPPERS_SNAPSHOT=1 cargo test -p metrics-aggregator --test wrappers`, refresh `monitoring/grafana/telemetry.json` (and related dashboard exports) to include the `bridge_remediation_ack_latency_seconds (p50/p95)` histogram, and document the dedup guarantees in your PR notes so operators know `bridge_remediation_ack_latency_seconds_count` remains idempotent across the restart boundary.

### Runtime Reactor

- `runtime_read_without_ready_total` increments when reads succeed without a readiness event (missed IO wakeups). Sustained growth indicates reactor/event-mapping issues.
- `runtime_write_without_ready_total` increments when writes succeed without a readiness event (missed IO wakeups). Sustained growth indicates reactor/event-mapping issues.

### Runtime Tuning Knobs

- `runtime.reactor_idle_poll_ms`: maximum sleep between reactor polls (ms).
- `runtime.io_read_backoff_ms`: fallback delay before retrying reads when readiness is missing (ms).
- `runtime.io_write_backoff_ms`: fallback delay before retrying writes when readiness is missing (ms).
- **Defaults:** `reactor_idle_poll_ms=100`, `io_read_backoff_ms=10`, `io_write_backoff_ms=10`.
- **When to tune:** Lower the idle poll and backoff values for latency-sensitive testnets; raise them when CPU is saturated by idle peers. Watch `runtime_read_without_ready_total` / `runtime_write_without_ready_total` and reactor CPU to validate changes.
- **BSD note:** On kqueue platforms we run level-triggered mode (no `EV_CLEAR`); if wakeups are missed, increase the backoff and ensure `update_interest()` instrumentation is present.
- **Env vars:** `TB_REACTOR_IDLE_POLL_MS`, `TB_IO_READ_BACKOFF_MS`, and `TB_IO_WRITE_BACKOFF_MS` map to the same knobs; config reloads apply without restart.

### Remote Signer Observability

- `remote_signer_discovery_total` increments whenever `wallet discover-signers` runs (default timeout 500 ms selectable via `--timeout`), giving you telemetry you can alert on when operator tooling probes the signer fleet.
- `remote_signer_discovery_success_total` increments when the discovery call returns at least one endpoint; chart these discovery counters next to `remote_signer_request_total`, `remote_signer_success_total`, and `remote_signer_latency_seconds` so you can tell if failures stem from networking vs. signer outages.
- `/wrappers` now mirrors the discovery counters; refresh `monitoring/tests/snapshots/wrappers.json` and the Grafana dashboards before merging any CLI or telemetry change so the new series stay comparable to what the aggregator exposes.
- The `Remote Signers` row that ships inside `monitoring/grafana/telemetry.json` now graphs `remote_signer_discovery_total`, `remote_signer_discovery_success_total`, and `remote_signer_discovery_success_ratio`, so you can visually correlate failed probes, recovered success, and the `/wrappers` hash without building the panel yourself. Re-run `npm ci --prefix monitoring && make monitor` (and capture the refreshed `/wrappers` hash) any time you adjust these metrics or their Grafana panels.
- Use `wallet discover-signers --json` for automation tooling; it emits `{"timeout_ms":<ms>,"signers":["http://..."]}` so CLI helpers and scripts can read a stable schema instead of parsing human text.

### P2P Rate Limiting and Chain Sync

| Knob | Default | Description |
|------|---------|-------------|
| `p2p_rate_window_secs` (`TB_P2P_RATE_WINDOW_SECS`) | 1 | Sliding window for request counters |
| `p2p_max_per_sec` (`TB_P2P_MAX_PER_SEC`) | workload-dependent | Max requests per peer per window |
| `p2p_max_bytes_per_sec` (`TB_P2P_MAX_BYTES_PER_SEC`) | workload-dependent | Max bytes per peer per window |
| `p2p_chain_sync_interval_ms` (`TB_P2P_CHAIN_SYNC_INTERVAL_MS`) | 500 | Periodic chain sync pull interval |

- Use narrow windows and lower maxima during incident response; widen for WAN drills. Keep `p2p_chain_sync_interval_ms` at 0 in isolation tests to prevent periodic pulls.

### Config Hot-Reload Fallback

- Config watcher prefers inotify/kqueue; when unavailable, it falls back to mtime polling. On platforms without reliable fs events, expect up to one poll interval of delay before reload. Documented in `node/src/config.rs`; operators can reduce the poll interval for faster propagation.

### TLS Handshake Timeouts

- HTTP servers use `ServerConfig.tls_handshake_timeout` for TLS handshakes.
- HTTP clients use `ClientConfig.tls_handshake_timeout` or `TlsConnectorBuilder::handshake_timeout`.
- Environment override: `TB_TLS_HANDSHAKE_TIMEOUT_MS` (milliseconds).

### Economics Autopilot Gate

- **Telemetry sources**
  - `economics_block_reward_per_block` shows the current base reward that Launch Governor was replaying when it evaluated economics.
  - `economics_prev_market_metrics_{utilization,provider_margin}_ppm` mirror the deterministic metrics derived from settlement receipts; these are the same samples that are held alongside the executor intent (`governor/decisions/epoch-*.json`) for audit.
  - `economics_epoch_tx_count`, `economics_epoch_tx_volume_block`, and `economics_epoch_treasury_inflow_block` capture the network activity, volume, and treasury inflow that feed the control loop.
  - `tb-cli governor status --rpc <endpoint>` prints the `telemetry gauges (ppm)` section plus the `last_economics_snapshot_hash`; that hash targets the JSON emitted by `economics::replay::replay_economics_to_tip`, so you can replay the same receipt-derived sample (tx counts, treasury inflow, and market metrics) that the governor evaluated.
  - Shadow mode is the default: set `TB_GOVERNOR_SHADOW_ONLY=1` to keep intents and snapshot hashes flowing without mutating runtime params, then flip it to `0` once the telemetry streak looks healthy to allow apply.
  - `tb-cli governor status` now also prints *release provenance* + *receipt health summary* blocks. Use the release provenance hash/attestation to confirm the governor signed the payload you just audited, and rely on the “hint” line to decide whether a rerun, attestation regen (`TB_GOVERNOR_SIGN=1` + `tb-cli governor snapshot --epoch N`), or rollback is required. Receipt health shows `signature_mismatch_total`, `header_mismatch_total`, `diversity_violation_total`, plus `validation/decoding` failures, pending-queue depth, and `receipt_drain_depth`; each hint points to the most effective remediation (provider key replay, shard root recalculation, reroute/backlog drain, or telemetry follow-up).
  - The same counters are exported in telemetry/grafana dashboards and `/wrappers`: `receipt_aggregate_sig_mismatch_total`, `receipt_header_mismatch_total`, `receipt_shard_diversity_violation_total`, `receipt_validation_failures_total`, `receipt_decoding_failures_total`, `pending_receipts_{storage,compute,energy}`, and `receipt_drain_depth` now appear in the aggregator snapshot so operators can alert on signature/header anomalies, diversity rebalance, backlog drain, and replay retrigger depth without touching the CLI. Refresh `monitoring/tests/snapshots/wrappers.json` (and `metrics-aggregator/tests/snapshots/wrappers.json`) plus `monitoring/grafana/telemetry.json` whenever you change these meters so the aggregator hash/panels remain in sync with the governor hints.

- **Auditing workflow**
  1. After collecting the metrics you expect (via Grafana or Prometheus), copy the hash from `tb-cli governor status`. Compare it against the Blake3 hash stored inside `governor/decisions/epoch-*.json` to ensure the governor replayed the same deterministic sample that the telemetry gauges exposed.
  2. Use `tb-cli governor intents --gate economics` to see pending intents and their `snapshot_hash` lines. Each hash should match the corresponding decision file in `governor/decisions/`.
  3. If you need to inspect the actual sample JSON, cat the decision file; it contains the metrics (`market_metrics`, `epoch_treasury_inflow`, etc.) that triggered the gate.
  4. When the CLI reports a release-provenance attestation mismatch or missing signature, run `tb-cli governor snapshot --epoch <N>` to re-export the payload, enable `TB_GOVERNOR_SIGN=1` + `TB_NODE_KEY_HEX=<key>` if missing, and repeat the audit until the hint reports “Attestation matches”. Keep the attestation JSON (`governor/decisions/epoch-*.json.sig`) in the same directory for deterministic replay.

- **Rollback play**
  1. Pause the governor by disabling it (`TB_GOVERNOR_ENABLED=0`) or shutting down the governor process; this prevents new intents from applying while you troubleshoot.
  2. If you were running in apply mode (`TB_GOVERNOR_SHADOW_ONLY=0`), flip back to shadow (`TB_GOVERNOR_SHADOW_ONLY=1`) so intents keep flowing for audit without mutating runtime parameters.
  3. To revert an applied gate, plan an exit intent (`GateAction::Exit`) by letting `tb-cli governor status` build up the required streak or by manually submitting the exit via the governor decision API. Confirm `economics_autopilot=false` in `tb-cli governor status`.
  4. Once the anomaly is addressed, re-enable the governor (`TB_GOVERNOR_ENABLED=1`) and replay the same metrics so the economics gate can re-enter from the known-good sample. Turn apply mode back on when you are ready to let the gate mutate runtime params again.

### Ad Market Quality + Cost Signals

- **Quality-adjusted pricing**
  - `ad_quality_multiplier_ppm{component}` reports freshness/privacy/readiness/overall multipliers (ppm).
  - `ad_quality_readiness_streak_windows` surfaces the readiness streak used for cohort quality.
  - `ad_quality_freshness_score_ppm` tracks the weighted freshness histogram score per presence bucket.
  - `ad_quality_privacy_score_ppm` tracks privacy budget headroom after denials/cooldowns.
- **Compute scarcity coupling**
  - `ad_compute_unit_price_usd_micros` is the compute-market spot price converted to USD micros.
  - `ad_cost_basis_usd_micros{component}` shows bandwidth/verifier/host/total floor components after scarcity coupling.
- **Tiered ad gates**
  - `ad_gate_ready_ppm{tier}` and `ad_gate_streak_windows{tier}` confirm contextual vs presence readiness streaks before apply.
  - Use `tb-cli governor status` to confirm the matching `ad_contextual`/`ad_presence` gate streaks and snapshot hashes.
- **Dashboards and wrappers**
  - Expand the `Ad Market Readiness` row with selector-level panels (`ad_auction_top_bid_usd_micros`, `ad_auction_win_rate`, `ad_bid_shading_factor_bps`, `ad_privacy_budget_utilization_ratio`, `ad_bidding_latency_micros`, `ad_conversion_value_total`) and refresh `monitoring/grafana/dashboard.json`, `monitoring/tests/snapshots/dashboard.json`, and the `/wrappers` hash after running `npm ci --prefix monitoring && make monitor`.
  - `/wrappers` now mirrors selector counters and the existing readiness/utilization gauges; keep `monitoring/tests/snapshots/wrappers.json` and `metrics-aggregator/tests/snapshots/wrappers.json` updated whenever the selector metrics change so the aggregated hash matches the dashboard export.
  - The explorer timeline surfaced by `ad_market.policy_snapshot` should log both the `CohortKeyV2` hash + `selectors_version` and the legacy `cohort_v1` tuple so operators can audit reversible migrations; record any timeline/schema tweaks in `docs/apis_and_tooling.md` and `docs/system_reference.md` before updating the corresponding explorer/CLI surfaces.
  - CI pins the governance treasury wrapper summary hash (`e6982a8b84b28b043f1470eafbb8ae77d12e79a9059e21eec518beeb03566595`) and the explorer/CLI treasury timeline schema hash (`c48f401c3792195c9010024b8ba0269b0efd56c227be9cb5dd1ddba793b2cbd1`); update the expected values only when intentionally changing the telemetry or response shape and refresh Grafana alongside.
- `monitoring/grafana_treasury_dashboard.json` is snapshot-checked in CI with hash `e9d9dc350aeedbe1167c6120b8f5600f7f079b3e2ffe9ab7542917de021a61a0`; regenerate with `make -C monitoring dashboard` and update snapshots/hash when panels change, and include refreshed screenshots in review notes.
- New storage-market discovery metrics (`storage_discovery_requests_total` and `storage_discovery_results_total{status=success|error}`) now feed the `/wrappers` summary so the dashboards can plot DHT query volumes and error ratios. Refresh `monitoring/tests/snapshots/wrappers.json` (and the matching `metrics-aggregator/tests/snapshots/wrappers.json` hash) and update the Grafana storage row after running `npm ci --prefix monitoring && make monitor` whenever these panels change so the aggregator/monitoring hash stays in sync with the CLI/RPC behavior.

### Range Boost + LocalNet Observability

- The Range Boost forwarder loop in `node/src/range_boost/mod.rs` enforces `MAX_FORWARD_RETRIES = 4` and the `RETRY_SLEEP`/`IDLE_SLEEP` pacing constants so every bundle either delivers, retries, or drops deterministically; if you change the budget you must update this section, the dashboards, and the `/wrappers` exporters that surface `range_boost_forwarder_*` counters.
- `range_boost_forwarder_retry_total` records every store-and-forward retry triggered by the Range Boost forwarder, and `range_boost_forwarder_drop_total` flags when bundles hit the deterministic retry limit and are dropped. Those counters sit alongside the existing `range_boost_forwarder_fail_total`, `range_boost_enqueue_error_total`, and queue-depth gauges inside the **Range Boost** row of `monitoring/grafana/dashboard.json` (and the mirrored `telemetry.json`, `dev.json`, `operator.json` exports). The panels plot the 5m delta for retries/drops so you can tell whether failures resolve on retry or linger.
- Entries from LocalNet receipts now surface `localnet_receipt_insert_attempt_total`, `localnet_receipt_insert_success_total`, and `localnet_receipt_insert_failure_total`, giving you observability into the bounded sled insert loop that retries on disk contention. Pair these counters with the queue-health panels above to spot whether the mesh is failing to persist attestations under load.
- The LocalNet insertion loop is governed by `LOCALNET_INSERT_RETRY_LIMIT = 3` and a fixed `LOCALNET_INSERT_RETRY_DELAY_MS = 25`, so any inserts that still fail after the budget raise `localnet_receipt_insert_failure_total` while the retry counter always matches the configuration; document any change to these knobs and refresh the dashboards / `/wrappers` snapshots alongside the Range Boost signals.
- `/wrappers` now mirrors `range_boost_*` and `localnet_receipt_*` counters; refresh both `monitoring/tests/snapshots/wrappers.json` and `metrics-aggregator/tests/snapshots/wrappers.json` (see the respective `WRITE_WRAPPERS_SNAPSHOT=1` helper commands) whenever you change their shape so dashboards and downstream tooling never see a drifted hash.
- Relay receipts emit `/wrappers` counters `relay_receipts_total`, `relay_receipt_bytes_total`, and `relay_job_rejected_total{reason}` so operators can spot when the carry-to-earn loop saturates the mesh or hits governance caps. These metrics feed the existing **Range Boost** row in `monitoring/grafana/dashboard.json`; add a panel that charts the 5-minute delta of `relay_receipts_total` and a companion gauge for `relay_receipt_bytes_total` so you can correlate throughput with the new relay receipt backlog.
- Shadow vs trade mode is controlled by `TB_RELAY_ECONOMICS_MODE` (`shadow` default, `trade` to start anchoring receipts in blocks). When you promote the gate, confirm that the Launch Governor intent or CLI flag states the same mode, note the `relay_*` counters in the next telemetry release, and document the switch in the runbook (see `docs/architecture.md#localnet-and-range-boost` for the spec). Shadow failures raise `relay_job_rejected_total{reason=...}` with reasons `payload_too_large`, `ack_stale`, or `budget_exhausted`; investigate those counters before letting trade mode pay carriers.
- Because `/wrappers` now advertises the `relay_*` counters, regenerate `monitoring/tests/snapshots/wrappers.json` and `metrics-aggregator/tests/snapshots/wrappers.json` via `WRITE_WRAPPERS_SNAPSHOT=1` (and update the pinned hashes below) whenever the metric set grows or splits so the Grafana dashboards, aggregator rollups, and CLI probes stay in sync.

### Transport handshake watchdog

- Dial attempts for every transport provider emit `transport_handshake_attempt_total{provider="<provider>"}` when the bounded handshake routine begins, whether it later succeeds or exhausts retries. This counter, combined with the existing `transport_provider_connect_total`, surfaces the “telescoping” behavior operators need to watch when retries back-pressure QUIC lanes. Add a panel next to the Range Boost row that charts the 5m delta of `transport_handshake_attempt_total`, and describe the metric inside the new `transport` row of `monitoring/grafana/dashboard.json`/`telemetry.json`.
- The same counter plus its “goal completion” sample appear in `/wrappers` so the aggregator can surface retry rates directly to the `render_foundation_dashboard.py` summary. Capture the refreshed `/wrappers` hash after you update the telemetry surface so Grafana snapshots and alerts align.

### Overlay persistence telemetry

- Overlay peer persistence now enforces a deterministic retry budget (`overlay_persist_attempts_total`) with success/failure counters (`overlay_persist_success_total`, `overlay_persist_failure_total`). Prime the overlay diagnostics panel in `monitoring/grafana/dashboard.json`/`telemetry.json` with these metrics so every restart shows whether the persisted peer store was able to survive write contention or if it dropped entries after the retry budget expired.
- `/wrappers` now contains the same persistence counters, giving you a ready-made signal for dashboards, probes, and runbooks that depend on stable discovery stores.

### Chaos drill artifacts

- Every chaos drill run inside `node/tests/chaos.rs` now writes a persistent artifact bundle (`log.txt` + metadata) plus a zipped archive under `target/chaos_artifacts/<test-name>-<timestamp>.zip` (override with `TB_CHAOS_ARTIFACT_ROOT`). The files capture loss/jitter knobs, convergence events, partition heal handoffs, and transport + Range Boost telemetry counters recorded during the drill. Share the zip path printed in the test logs when you triage failures so replays can start from the exact scenario that produced the alert.
- Replay drills should cite the artifact hash along with the Grafana panels mentioned above; when you rerun `npm ci --prefix monitoring && make monitor --native-monitor`, make sure the refreshed dashboards continue referencing the new panels so operators know which metric to inspect when an artifact is published.

### Energy RPC Guardrails

- RPC calls enforce optional auth (`TB_ENERGY_RPC_TOKEN`) and a sliding-window rate limit (`TB_ENERGY_RPC_RPS`, default 50rps). Missing/incorrect tokens return `-33009`, while limits return `-33010`.
- Auth and rate checks happen before parsing business parameters, so rate spikes on unauthenticated traffic show up as `energy_signature_verification_failures_total` and the aggregator summary’s energy section.
- Aggregator `/wrappers` now includes `energy.rate_limit_rps` so dashboards can display the configured limit alongside dispute/settlement health.
- Keep these values in sync with downstream dashboards: the aggregator exposes `energy_*` counters in `/wrappers` and the Grafana energy board charts dispute counts, settlement backlog (`energy_pending_credits_total`), and signature failures.
- The energy dashboards also surface the new `energy_quorum_shortfall_total`, `energy_reading_reject_total{reason}`, and `energy_dispute_total{state}` counters so operators can correlate rejected readings, quorum shortfalls, and dispute lifecycle movements with the aggregator summary and Prometheus alerts.
- When you harden energy receipts/disputes (quorum/expiry updates, new slash rules, explorer timeline wiring), document the rollout via:
  1. Publishing the current `governance/energy/settlement/history` entry for the change plus any rollback record (`contract-cli gov energy-settlement --timeline`).
  2. Capturing the refreshed `/wrappers` hash and listing the new `energy_pending_credits_total`, `energy_active_disputes_total`, and `energy_slashing_total` gauges so downstream tooling (alert rules, dashboards) can pin the update.
     - The canonical `/wrappers` hash for this metric set is `21ba9ccb7807b26a0696181f1fcef54a35accf1cd4064e6d6ed38d4a36e197cb`; regenerate it with `WRITE_WRAPPERS_SNAPSHOT=1 cargo test --manifest-path monitoring/Cargo.toml --test wrappers` and cite that hash when closing the story.
  3. Updating the Grafana energy board screenshots (new Active Disputes and Cumulative Slashes gauges) and noting whether operators should expect new explorer timelines (`/governance/energy/slashes`, `/governance/energy/settlement/history`) before/after the change.
     - Capture the refreshed panel(s) by visiting `monitoring/output/index.html` (produced via `make monitor -- --native-monitor`) and keeping the screenshot next to the rollout notes so reviewers can compare against the archived panels.
- Operators now have a `/governance/energy/timeline` endpoint (queryable by `provider_id`, `event_type`, and `meter_hash`) that surfaces `receipt`, `dispute_opened`, `dispute_resolved`, and `slash` events along with the recorded block/metadata so they can trace meter-to-receipt flows before digging into disputes or slashes.

## Gateway Service Runbook

- **Service binary & unit** – `deploy/systemd/gateway.service` now points at the first-class `gateway-service` binary (`cmd/gateway-service.rs`), which runs `the_block::web::gateway::run` and exposes `TB_GATEWAY_INTERFACES`, `TB_GATEWAY_PORT`, and `TB_GATEWAY_BLOCK_NAME`. Production deploys should align the unit file, Grafana dashboard, and TLS hooks with this binary; local troubleshooting can still use `cargo run -p gateway -- run` but the service must match the shipped binary.
- **TLS wiring** – TLS is configured either through the CLI flags `--tls-cert`, `--tls-key`, `--tls-client-ca`, and `--tls-client-ca-optional`, or by setting the matching env vars `TB_GATEWAY_TLS_CERT`, `TB_GATEWAY_TLS_KEY`, `TB_GATEWAY_TLS_CLIENT_CA`, and `TB_GATEWAY_TLS_CLIENT_CA_OPTIONAL`. The gateway respects the `http_env` naming pattern so anything that already stages `TB_*_TLS` for RPC or aggregator can be reused.
- **Stake gating** – HTTP requests must include a `Host` header that resolves to a domain with a stake deposit. The gateway ignores any port suffix (so `Host: example.block:9000` still maps to `example.block`) before checking the DNS ownership store (`node/src/gateway/dns.rs`) and verifying that `dns_ownership/<domain>` points to a `dns_stake/<reference>` record with positive escrowed BLOCK—the helper `domain_has_stake` enforces this check before any content is served. Use the DNS auction/stake CLI (see the same file) to mint the domain and deposit the stake.
- **Static blobs** – Static files live under `pipeline/gateway/static/<domain>/<path>`. The request path is sanitized (`pipeline::sanitized_path`), so populate the matching directory tree within `pipeline/gateway/static` and the gateway will serve it directly (see `node/src/storage/pipeline.rs:fetch_blob`).
- **Smoke test** – When you deploy a gateway, verify the stake gate with `curl http://localhost:9000/ -H "Host: some.block"`: expect `403 domain stake required` before the domain entry is funded, then `200 OK` once `dns_ownership/some.block` includes an `owner_stake`. The integration test at `node/tests/gateway_service.rs` exercises this exact flow.

### `.block` DNS resolver (DoH)

- See [`docs/gateway_mobile_resolution.md`](docs/gateway_mobile_resolution.md) for the phone-specific DoH/DNS runbook, TLS knobs, and verification steps referenced below.

- **Bridge domain ownership** – `TB_GATEWAY_BLOCK_NAME` should match the authorized `.block` domain advertised via the DoH resolver (`TB_GATEWAY_RESOLVER_CNAME`). The gateway acts as the authoritative DNS server/DoH provider for that domain by proxying requests only when `dns_ownership/<domain>` and `dns_stake/<reference>` show positive escrow, ensuring mobile devices always resolve legitimate `.block` hosts through the in-house stack.

- **Purpose** – The gateway now speaks DNS-over-HTTPS at `/dns/resolve`. The endpoint returns `application/dns-json` payloads with `Status`, TTL, and `Answer` arrays, only responds to `.block` domains, and reuses the same stake table that gates static hosts. The behavior is driven by three knobs:
  - `TB_GATEWAY_RESOLVER_ADDRS`: comma-separated IPv4/IPv6 addresses the resolver should advertise (default: empty in production; for localhost smoke tests where `TB_GATEWAY_URL` points at a loopback address, the gateway will advertise that loopback IP by default).
  - `TB_GATEWAY_RESOLVER_TTL`: cache TTL in seconds (default `60`). The gateway echoes this value in the JSON `Answer` entries and the HTTP `Cache-Control` header.
  - `TB_GATEWAY_RESOLVER_CNAME`: optional CNAME target emitted when the address list is empty (for example `gateway.example.block` pointing back into the mesh).

- **Device setup** – Android users can configure “Private DNS” or “Custom DoH” and point it to `https://<gateway>/dns/resolve?name=%s&type=%t` with a wrapped hostname such as `gateway.example.block`. iOS/macOS clients can use the DNS settings in `Settings → Wi-Fi → Configure DNS → Manual` with a third-party DoH profile (e.g., NextDNS) that lets you specify the same URL template; browsers accept the same string via their DoH settings. Desktop apps that understand the DoH JSON format will simply call `GET /dns/resolve?name=foo.block&type=A`.
- **Smoke test** – Run `curl -v https://gateway.example.block/dns/resolve?name=foo.block&type=A`. If the domain has stake, you should see HTTP `200`, `Status=0` in the JSON, at least one `Answer` entry, and a `Cache-Control` header matching the TTL. Removing the stake record should flip the HTTP status to `403 domain stake required` while the JSON body still emits `Status=3`.
- **Failure handling** – Stake shortages and unanswered questions both emit HTTP `Status=3` payloads so aggregator counters stay aligned; a 403 response still includes an empty JSON `Answer` array (and `cache-control: max-age=0`), which keeps `aggregator_doh_resolver_failure_total` ticking even when the request was rejected before serving DNS data.
- **Chaos drill** – Simulate resolver drift by temporarily blanking `TB_GATEWAY_RESOLVER_ADDRS` or pointing it at a non-routable IP. Confirm clients fail fast or fall back to a cached `TB_GATEWAY_URL` share link, then re-apply the previous settings and ensure the same JSON output returns within one TTL so the failure window is captured in the incident log.
- **Telemetry & alerts** – Each `Status=3` response bumps `aggregator_doh_resolver_failure_total` in `/wrappers`, and the `DnsResolverStatus3Detected` alert in `monitoring/alert.rules.yml` reacts to the 5m delta so you know when resolver answers disappear. The `monitoring/grafana/telemetry.json` dashboard now exposes a panel for `aggregator_doh_resolver_failure_total` so you can correlate resolver failures with stake-table/gateway changes; capture a new panel screenshot and `/wrappers` hash whenever you touch this surface.

### Drive-lite file loop

- **Purpose** – `contract-cli storage put <file>` now uploads real bytes, prints the object ID, and emits a shareable URL (`TB_GATEWAY_URL/drive/<object_id>`). The gateway exposes `/drive/:object_id`, which serves cached bytes from `blobstore/drive` and optionally fetches missing IDs from trusted peers (`TB_DRIVE_PEERS`). Two machines can share a file by setting one node’s `TB_DRIVE_PEERS` to the other’s gateway URL while retaining determinism and replay integrity.
- **Configuration**
  - `TB_DRIVE_BASE_DIR`: where the gateway caches objects (default `blobstore/drive`). Ensure the path exists and is mirrored by your `.gitignore`.
  - `TB_DRIVE_PEERS`: comma-separated fallback URLs (e.g., `https://gateway2.example.block`). When a requested ID is missing locally, the gateway sequentially queries these peers before returning `404`.
  - `TB_DRIVE_ALLOW_PEER_FETCH`: enable remote fetching (default `1`). Use `0` for air-gapped or single-node deployments.
  - `TB_DRIVE_FETCH_TIMEOUT_MS`: HTTP timeout for peer fetches (default `3000` ms).
  - `TB_GATEWAY_URL`: base URL (default `http://localhost:9000`) used by the CLI when printing the share link and by metrics that expose the drive endpoint.
  - Stake gating still applies: the `Host` header must point at a domain with `domain_has_stake`, so `/drive/<id>` reuses the same deposit you already maintain for static assets.
- **Workflow**
  1. Run `contract-cli storage put /path/to/file`. The CLI prints the object ID and the share link, and honors `--deterministic-fixture` for reproducible testdata.
  2. Retrieve the object locally with `curl --fail https://yourgateway/drive/<object_id>`. Expect `Content-Type: application/octet-stream` and `Content-Length` equal to the file size.
  3. For cross-node reads, configure the secondary node with `TB_DRIVE_PEERS=https://primary.gateway/block` and `TB_DRIVE_ALLOW_PEER_FETCH=1`. The secondary portal will fetch the bytes over HTTPS, cache them under its own `blobstore/drive`, and serve them for subsequent requests.
  4. Share the `TB_GATEWAY_URL` link (e.g., https://gateway.example.block/drive/<object_id>) in release notes or DoH TXT records so mobile browsers can open the resource directly.
- **Smoke test** – Upload a sample file, fetch it from the same gateway, then switch to another node with `TB_DRIVE_PEERS` pointing at the uploader and confirm `curl` succeeds there too. Log the `contract-cli` output and the `curl` response (match the hashes) before and after toggling `TB_DRIVE_ALLOW_PEER_FETCH` to prove the fallback path works.
- **Chaos drill** – Stop the peer that holds the file while the secondary node is fetching; the request should fail with a `404` or `504` depending on timeout. Capture the failure and rerun the upload + fetch sequence once the peer rejoins so you can compare the drive cache timestamps and confirm the recovery path behaves deterministically.

## Treasury Stuck

Payload alignment and telemetry:
- RPC/CLI/Explorer now surface `expected_receipts` and a canonical `deps` list (proposal `deps` preferred; memo hints capped at 100 entries, memo size limited to 8KiB, destinations must start with `tb1`).
- `/wrappers` exports treasury executor gauges (`treasury_executor_pending_matured`, `treasury_executor_staged_intents`, `treasury_executor_lease_released`) so dashboards can alert on queue depth and lease state alongside `treasury_disbursement_backlog`.
- When dashboards change, re-run `/wrappers` and capture the updated hash in review notes so operators can verify they scraped the latest governance/treasury snapshot.

### Symptoms

- [ ] `treasury_disbursement_backlog > 50` for 2+ epochs
- [ ] `treasury_disbursement_lag_seconds_p95 > 300`
- [ ] Executor reports `last_error != null`
- [ ] Stale disbursements (created_at > 3 days ago, still Queued)
- [ ] No state transitions occurring

### Diagnosis

**Step 1**: Check executor health

```bash
contract-cli gov treasury balance | jq .executor
# Look for:
#   last_error: string (should be null)
#   last_success_at: recent timestamp
#   pending_matured: count of ready-to-execute
#   lease_holder: node_a (should be active node)
#   lease_expires_at: future timestamp
```

**Step 2**: List stuck disbursements

```bash
echo "=== Disbursements stuck for >1 hour ==="
contract-cli gov treasury list --limit 200 --status queued | jq \
  '.disbursements[] | select(.created_at < (now - 3600)) | {id, created_at, updated_at}'

echo "=== Count stuck disbursements ==="
contract-cli gov treasury list --limit 200 --status queued | jq \
  '.disbursements | length'
```

**Step 3**: Check for dependency failures

```bash
echo "=== Queued disbursements with dependencies ==="
contract-cli gov treasury list --status queued --limit 50 | while read id; do
  deps=$(contract-cli gov treasury show --id "$id" | jq '.proposal.deps')
  if [ "$deps" != "[]" ]; then
    echo "ID $id depends on: $deps"
    # Check if dependencies are satisfied
    echo "$deps" | jq '.[]' | while read dep_id; do
      state=$(contract-cli gov treasury show --id "$dep_id" | jq -r '.status.state')
      echo "  ↳ Dependency $dep_id: $state"
    done
  fi
done
```

**Step 4**: Inspect error logs

```bash
echo "=== Recent treasury executor errors ==="
grep "treasury_executor\|DisbursementError" /var/log/node/*.log \
  | grep -E "ERROR|WARN" \
  | tail -50

echo "=== Executor tick duration ==="
prometheus_query 'histogram_quantile(0.99, treasury_executor_tick_duration_seconds_bucket)'

echo "=== Execution error rate ==="
prometheus_query 'rate(treasury_execution_errors_total[5m])'
```

**Step 5**: Check treasury balance

```bash
contract-cli gov treasury balance | jq '{balance}'

# If insufficient, show what's waiting to execute
echo "=== Pending disbursements (sum of amounts) ==="
contract-cli gov treasury list --status queued --limit 200 | jq \
  '.disbursements | map(.amount) | add'
```

### Resolution Paths

#### **Path A: Dependency Issue**

**Indicators**: Dependencies in wrong state

```bash
# List dependencies and their states
contract-cli gov treasury show --id <STUCK_ID> | jq '{id: .id, deps: .proposal.deps}'

# For each dependency, check state
for dep_id in $(contract-cli gov treasury show --id <STUCK_ID> | jq -r '.proposal.deps[]'); do
  state=$(contract-cli gov treasury show --id "$dep_id" | jq -r '.status.state')
  echo "Dependency $dep_id: $state"
  
  if [ "$state" = "rolled_back" ]; then
    echo "  ✗ Dependency failed. Cancelling dependent..."
    # Cancel the stuck disbursement
    contract-cli gov treasury rollback --id <STUCK_ID> \
      --reason "Dependency $dep_id was rolled back"
  fi
done
```

#### **Path B: Insufficient Funds**

**Indicators**: `treasury_execution_errors_total{reason="insufficient_funds"}` increasing

```bash
# Check current balance
current=$(contract-cli gov treasury balance | jq .balance)
pending=$(contract-cli gov treasury list --status queued | jq '[.disbursements[].amount] | add')

echo "Current balance: $current BLOCK"
echo "Pending disbursements: $pending BLOCK"

if [ $current -lt $pending ]; then
  echo "INSUFFICIENT FUNDS"
  echo "Wait for accruals: watch -n 30 'contract-cli gov treasury balance | jq .balance'"
  echo "OR request governance approval for fund allocation"
fi
```

#### **Path C: Executor Error**

**Indicators**: `last_error != null`, executor not progressing

```bash
# Get the exact error
error=$(contract-cli gov treasury balance | jq -r '.executor.last_error')
echo "Executor error: $error"

# Common errors and fixes:
case "$error" in
  "ledger_unavailable")
    echo "Ledger connection lost. Check: systemctl status ledger-node"
    ;;
  "consensus_timeout")
    echo "Consensus stalled. Check: contract-cli node consensus --status"
    ;;
  "dependency_cycle_detected")
    echo "Circular dependency found. Review recent disbursement submissions."
    ;;
  *)
    echo "Unknown error. Check logs: tail -100 /var/log/node/*.log"
    ;;
esac

# Restart executor
echo "Attempting executor restart..."
sudo systemctl restart the-block

# Monitor recovery
echo "Monitoring backlog recovery..."
watch -n 5 'contract-cli gov treasury balance | jq .executor.last_error'
```

#### **Path D: Data Corruption**

**Indicators**: Multiple disbursements stuck with inconsistent states

```bash
# Snapshot current state
contract-cli gov treasury list --limit 500 > /tmp/disburse_snapshot_$(date +%s).json
grep "treasury_executor" /var/log/node/*.log > /tmp/executor_logs_$(date +%s).txt

# Contact operations with:
echo "Send to ops team:"
echo "  - /tmp/disburse_snapshot_*.json"
echo "  - /tmp/executor_logs_*.txt"
echo "  - prometheus_dump.json (from curl http://localhost:9090/api/v1/query_range?...)"
echo "  - Describe when issue started"
```

### Alert Thresholds

**CRITICAL** (Page on-call):
```promql
# Backlog accumulating
treasury_disbursement_backlog > 100 for 3 epochs

# Executor failing
rate(treasury_execution_errors_total[5m]) > 1

# No progress for extended period
increase(governance_disbursements_total{status="finalized"}[10m]) == 0
```

**WARNING** (Create ticket):
```promql
# High latency
histogram_quantile(0.95, treasury_disbursement_lag_seconds_bucket) > 600

# Moderate backlog
treasury_disbursement_backlog > 50 for 2 epochs
```

---

## Energy Stalled

### Symptoms

- [ ] `oracle_latency_seconds_p95 > 10`
- [ ] `energy_signature_verification_failures_total` increasing rapidly (> 1/min)
- [ ] `energy_pending_credits_total` not decreasing
- [ ] Provider readings not settling
- [ ] Disputes backlog increasing

### Diagnosis

**Step 1**: Check oracle latency

```bash
echo "=== Oracle latency distribution ==="
prometheus_query 'histogram_quantile(0.50, oracle_latency_seconds_bucket)'
prometheus_query 'histogram_quantile(0.95, oracle_latency_seconds_bucket)'
prometheus_query 'histogram_quantile(0.99, oracle_latency_seconds_bucket)'

echo "=== Oracle process status ==="
sudo systemctl status energy-oracle
grep oracle_latency /var/log/oracle/*.log | tail -20
```

**Step 2**: Check signature verification failures

```bash
echo "=== Signature failure rate ==="
prometheus_query 'rate(energy_signature_verification_failures_total[5m])'

echo "=== Recent verification failures ==="
grep "signature_verification_failed\|SignatureInvalid" /var/log/node/*.log | tail -30

echo "=== Breakdown by reason ==="
for reason in invalid_format verification_failed key_not_found scheme_unsupported; do
  count=$(prometheus_query "energy_signature_verification_failures_total{reason=\"$reason\"}" | jq '.data.result[0].value[1]')
  echo "  $reason: $count"
done
```

**Step 3**: Check meter reading accumulation

```bash
echo "=== Pending credits (kWh) ==="
contract-cli energy credits list --status pending --limit 50 | jq \
  '.credits | {total_kwh: map(.amount_kwh) | add, count: length}'

echo "=== Credits by provider ==="
contract-cli energy credits list --status pending --limit 200 | jq \
  '.credits | group_by(.provider_id) | map({provider: .[0].provider_id, count: length, total_kwh: map(.amount_kwh) | add})'
```

**Step 4**: Check for timestamp issues

```bash
echo "=== Timestamp skew errors ==="
grep "timestamp_skew\|TimestampSkew" /var/log/node/*.log | wc -l

echo "=== System clock status ==="
timedatectl
ntpq -p

echo "=== Provider clock differences ==="
for provider in $(contract-cli energy market | jq -r '.providers[].provider_id' | head -10); do
  last_reading=$(contract-cli energy provider show "$provider" | jq .last_settlement)
  echo "$provider: $last_reading"
done
```

**Step 5**: Check provider status

```bash
echo "=== Inactive providers ==="
contract-cli energy market | jq '.providers[] | select(.status != "active") | {provider_id, status, reputation_score}'

echo "=== Providers with poor reputation ==="
contract-cli energy market | jq '.providers[] | select(.reputation.composite_score < 0.5) | {provider_id, score: .reputation.composite_score}'
```

### Resolution Paths

#### **Path A: Oracle Latency High**

**Indicators**: p95 latency > 10 seconds

```bash
# Check oracle CPU and memory
ps aux | grep oracle
top -p $(pgrep -f oracle)

# Check oracle queue depth
grep "pending_verifications\|queue_depth" /var/log/oracle/*.log | tail -10

# Scale oracle if needed (multiple instances)
echo "Consider: kubectl scale deployment energy-oracle --replicas=3"

# Monitor improvement
watch -n 5 'prometheus_query "histogram_quantile(0.95, oracle_latency_seconds_bucket)"'
```

#### **Path B: Signature Verification Failures**

**Indicators**: Failures > 1/min, reason = "verification_failed" or "invalid_format"

```bash
# Check provider keys
echo "=== Checking oracle key manager ==="
contract-cli energy oracle keys status

# Verify provider public keys in registry
for provider in $(contract-cli energy market | jq -r '.providers[].provider_id' | head -5); do
  key=$(contract-cli energy provider show "$provider" | jq .public_key)
  echo "$provider: $key"
done

# Test a signature manually
echo "=== Testing signature generation ==="
cat > /tmp/test_sig.py << 'EOF'
import ed25519, struct, base64, time

provider_id = "provider_usa_001"
meter = "meter_001"
total_kwh = 1500000
timestamp = int(time.time())
nonce = 12345

# Build message
message = (
    provider_id.encode() +
    meter.encode() +
    struct.pack('<Q', total_kwh) +
    struct.pack('<Q', timestamp) +
    struct.pack('<Q', nonce)
)

# Test signature
signing_key = ed25519.SigningKey(base64.b64decode("<PRIVATE_KEY>"))
signature = signing_key.sign(message).signature
print(f"Signature: {base64.b64encode(signature)}")
EOF
python /tmp/test_sig.py
```

#### **Path C: Timestamp Skew**

**Indicators**: "timestamp_skew" errors in logs

```bash
# Check system clock on provider and oracle
echo "=== Provider nodes ==="
for node in provider_node_1 provider_node_2 oracle_node; do
  echo "$node:"
  ssh "$node" 'date; timedatectl'
done

# Fix time drift
echo "Syncing clocks with NTP..."
sudo ntpdate -u ntp.ubuntu.com  # or your NTP server

# Verify sync
timedatectl
for node in provider_node_1 provider_node_2; do
  ssh "$node" 'timedatectl'
done

# Monitor recovery
watch -n 10 'grep timestamp_skew /var/log/node/*.log | tail -5'
```

#### **Path D: Provider Reputation Degradation**

**Indicators**: Multiple providers with score < 0.5

```bash
# Check what caused reputation drops
echo "=== Recent disputes ==="
contract-cli energy disputes list --limit 20

# Check slashing events
echo "=== Recent slashing ==="
prometheus_query 'rate(energy_slashing_total[24h])' | jq '.data.result[] | {provider, reason: .metric.reason, rate: .value}'

# Review evidence
for dispute_id in $(contract-cli energy disputes list --status resolved | jq -r '.disputes[].dispute_id' | head -5); do
  echo "Dispute $dispute_id:"
  contract-cli energy disputes show --id "$dispute_id"
done
```

### Alert Thresholds

**CRITICAL** (Page on-call):
```promql
# Oracle broken
oracle_latency_seconds_p95 > 30
energy_signature_verification_failures_total > 10

# Settlement stalled
increase(energy_settlements_total[10m]) == 0

# Dispute backlog critical
energy_active_disputes_total > 50
```

**WARNING** (Create ticket):
```promql
oracle_latency_seconds_p95 > 10
rate(energy_signature_verification_failures_total[5m]) > 1
energy_active_disputes_total > 20
```

---

## Receipts Flatlining

### Symptoms

- [ ] `receipt_emitted_total` flat or decreasing for 5+ blocks
- [ ] One or more markets (storage, compute, energy, ad) not emitting
- [ ] `receipt_validation_errors_total` increasing
- [ ] Explorer receipt tables not updating

### Diagnosis

### Provider key provenance & replay guard

- Provider keys now live inside the ledger snapshot (`ChainDisk.provider_registry`). Each entry records the `ProviderRegistrationSource` (config path, governance intent, signed announcement, stake-linked policy), the region/ASN hints used for shard diversity, and the full `ProviderKeyVersion` history with `registered_at_block`/`retired_at_block` timestamps. Decode the latest `chain_db/chain` (the same payload you inspect for treasury audits) to confirm where the key came from and whether it is still active for the block range you are replaying.
- Replay prevention depends on the per-receipt `signature_nonce` and the `receipt_crypto::NonceTracker` that records every `(provider_id, nonce)` pair for the configured `RECEIPT_NONCE_FINALITY` window. A `replayed_nonce` error means you are seeing a duplicate nonce within the finality window; look up the offending provider in the persisted registry to see which key was active at the time and whether a rotation happened since.
- When a receipt header reports `receipt_header_mismatch`/`receipt_aggregate_sig_mismatch`, recompute the BLAKE3 digest encoded by `ReceiptAggregateScheme::BatchEd25519`: aggregate the per-shard leaf hashes, write each signature length + bytes, and run the same `receipt_crypto::aggregate_signature_digest` routine to confirm every node sees the same commitment even though we do not yet have a true aggregated signature backend.

**Step 1**: Check emission rates by market

```bash
echo "=== Receipt emission rate (1m) ==="
prometheus_query 'rate(receipt_emitted_total[1m])' | jq '.data.result[] | {market: .metric.market, rate: .value[1]}'

echo "=== Markets with no emissions (last 10 blocks) ==="
prometheus_query 'increase(receipt_emitted_total[10m]) == 0' | jq '.data.result[].metric'
```

**Step 2**: Check validation errors

```bash
echo "=== Validation error rate ==="
prometheus_query 'rate(receipt_validation_errors_total[5m])'

echo "=== Error breakdown by reason ==="
for reason in schema_mismatch duplicate_detection signature_invalid; do
  count=$(prometheus_query "receipt_validation_errors_total{reason=\"$reason\"}" | jq '.data.result[0].value[1]')
  echo "  $reason: $count"
done
```

**Step 3**: Per-market diagnostics

```bash
# Storage market
echo "=== Storage market ==="
contract-cli receipts stats --market storage
grep storage /var/log/node/receipts.log | tail -20

# Compute market
echo "=== Compute market ==="
contract-cli receipts stats --market compute
grep compute /var/log/node/receipts.log | tail -20

# Energy market
echo "=== Energy market ==="
contract-cli receipts stats --market energy
grep energy /var/log/node/receipts.log | tail -20

### Slash & rejection monitoring

```bash
echo "=== Energy slash receipts ==="
contract-cli energy slashes --provider-id energy-0x00 --json

echo "=== Quorum shortfall rate (5m) ==="
prometheus_query 'rate(energy_quorum_shortfall_total[5m])'

echo "=== Reading rejection rate (5m) ==="
prometheus_query 'rate(energy_reading_reject_total[5m])'
```

The Grafana energy dashboard now surfaces these metrics (quorum shortfalls, reading rejects, dispute states) alongside the existing slash/settlement panels so operators can correlate telemetry with ledger events.

# Ad market
echo "=== Ad market ==="
contract-cli receipts stats --market ad
grep ad /var/log/node/receipts.log | tail -20
```

**Step 4**: Check block height and progression

```bash
echo "=== Current block ==="
contract-cli node status | jq '.current_block_height'

echo "=== Block progression (last 50 blocks) ==="
prometheus_query 'increase(block_height_total[50m])'

echo "=== Consensus status ==="
contract-cli node consensus --status
```

### Resolution

**For stalled markets**:

```bash
# Restart receipt emitter
sudo systemctl restart receipt-emitter

# Monitor recovery
watch -n 5 'contract-cli receipts stats --market storage'

# If persists, check market-specific service
for market in storage compute energy ad; do
  sudo systemctl status "${market}-market"
done
```

**For validation errors**:

```bash
# Clear validation state (if safe)
contract-cli receipts reset-validation-state

# Re-validate last N blocks
contract-cli receipts validate --from-block $((
  $(contract-cli node status | jq .current_block_height) - 100
))

# Monitor
watch -n 10 'prometheus_query "receipt_validation_errors_total"'
```

---

## Explorer Treasury Schema Migration

Run this playbook whenever the explorer SQLite database still contains the legacy `amount`/`amount_it` columns in `treasury_disbursements`.

1. **Stop explorer** so the migration can take an exclusive lock on the DB file.
2. Run the helper (defaults to `explorer.db` in the current directory):
   ```bash
   cargo run -p explorer --bin explorer-migrate-treasury -- /var/lib/explorer/explorer.db
   ```
   The tool applies the three `ALTER TABLE` statements (`ADD COLUMN status_payload`, `RENAME COLUMN amount TO amount`, `DROP COLUMN amount_it`). Statements that have already landed are reported as `skipped`.
3. Restart explorer, then validate `/governance/treasury/disbursements` and the treasury dashboards before announcing completion.

---

## Settlement Audit

### How to Run

```bash
# Standard settlement audit
cargo test -p the_block --test settlement_audit --release -- --nocapture

# With specific options
STARTING_EPOCH=0 ENDING_EPOCH=1000 cargo test \
  -p the_block --test settlement_audit --release -- --nocapture

# Verbose output
RUST_LOG=debug cargo test -p the_block --test settlement_audit --release -- --nocapture
```

### Interpreting Results

**Successful audit**:
```
test settlement_audit ... ok

Ledger conservation verified:
  Initial balance: 10,000,000 BLOCK
  Accruals: 1,500,000 BLOCK
  Executed disbursements: 2,000,000 BLOCK
  Final balance: 9,500,000 BLOCK
```

**Failed audit** (example):
```
test settlement_audit ... FAILED

Assertion failed:
  Expected balance: 9,500,000 BLOCK
  Actual balance: 9,300,000 BLOCK
  Discrepancy: 200,000 BLOCK (2.1%)

Investigation:
  1. Find missing disbursement: ID 4521
  2. Check status: Executed but not credited
  3. Verify: Receipt exists? Yes. Target account? Valid.
  4. Root cause: Ledger index out of sync
```

### Troubleshooting

**If audit fails**:

```bash
# Get detailed logs
cargo test -p the_block --test settlement_audit --release -- --nocapture --test-threads=1 2>&1 | tee settlement_audit.log

# Extract specific disbursement details
grep "Disbursement 4521" settlement_audit.log

# Check ledger state
contract-cli node ledger inspect --account treasury_account_id

# Verify receipts associated with missing disbursement
grep "disbursement_id.*4521" /var/log/node/*.log
```

---

## Helper Functions

### prometheus_query()

**Purpose**: Query Prometheus for metric values

**Usage**:
```bash
prometheus_query 'up{instance="localhost:9090"}'
prometheus_query 'histogram_quantile(0.95, request_duration_seconds_bucket)'
```

**Implementation**:
```bash
prometheus_query() {
  local query="$1"
  local url="${PROMETHEUS_URL:-http://localhost:9090}"
  curl -s "${url}/api/v1/query" \
    --data-urlencode "query=${query}" | \
    jq -r '.data.result[0].value[1] // "no data"'
}
```

**Configuration**:
```bash
export PROMETHEUS_URL="http://prometheus.infra.internal:9090"
```

---

## SLO Definitions

### Treasury System

| Metric | Target | Alert Threshold |
|--------|--------|----------|
| Availability | 99.95% | Errors > 0.1% for 5 min |
| Execution Latency p95 | < 300s | > 600s for 10 min |
| Error Rate | < 0.1% | > 1 error/sec |
| Queue Depth | < 100 | > 100 for 3 epochs |

### Energy System

| Metric | Target | Alert Threshold |
|--------|--------|----------|
| Oracle Latency p95 | < 5s | > 10s for 5 min |
| Signature Validation | > 99.9% | > 1 failure/min |
| Settlement Rate | > 95% | < 90% for 10 min |
| Dispute Resolution | < 1 hour | Unresolved > 2 hours |

Track `energy_settlement_mode` (0=batch,1=real_time) and `energy_settlement_rollback_total` in Prometheus/aggregator dashboards; rollbacks leave a persistent entry in the explorer’s `/governance/energy/settlement/history` endpoint and can be previewed via `contract-cli gov energy-settlement --timeline`.

### Receipts System

| Metric | Target | Alert Threshold |
|--------|--------|----------|
| Emission Rate | All markets | Any market = 0 for 5 blocks |
| Validation Success | > 99.99% | < 99% for 10 min |
| Storage | All receipts | Storage > 1 week |
| Query Latency p99 | < 100ms | > 500ms for 5 min |

---

**Last Updated**: 2025-12-19  
**Next Review**: 2025-12-26  
**Maintainer**: Operations Team  
