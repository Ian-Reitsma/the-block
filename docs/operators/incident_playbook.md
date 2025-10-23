# Incident playbook
> **Review (2025-09-25):** Synced Incident playbook guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

## Convergence lag
- Run `just probe:tip` and inspect `gossip_convergence_seconds`.
- Inspect peers via `logs` and ensure feature bits match.
- Gather `just support:bundle` and attach to ticket.

## High consumer fees
- Check proposals adjusting `ConsumerFeeComfortP90Microunits`.
- Review consumer `mempool` pressure and pending activations.
- Consider proposing a higher comfort threshold.

## Industrial stalls
- Inspect `admission_rejected_total{reason=*}` and `record_available_shards`.
- Adjust `IndustrialAdmissionMinCapacity` or quotas.

## Data corruption
- Watch `price_board_load_total{result="corrupt"}`; node auto-recovers.
- If repeated, replace disk after taking a support bundle.

## Read-denial spikes
- Monitor `read_denied_total{reason}` for sudden increases.
- Verify token-bucket settings in `gateway/http.rs` and domain DNS policy.
- Ensure clients are not exceeding documented traffic limits.

## Bridge liquidity remediation
- Watch the bridge row in Grafana/HTML snapshots: the remediation panels display
  both the action/playbook pair and dispatch outcomes alongside
  `bridge_liquidity_*` asset deltas and the new annotation-aware response text.
- When an action fires, inspect `/remediation/bridge` for the persisted entry
  and `/remediation/bridge/dispatches` for the per-target delivery log. The
  payload now includes `annotation`, `dashboard_panels`, and a
  `response_sequence` summarising the exact steps the CLI/ops automation expects
  you to follow.
- Confirm the JSON payload has been dispatched to the configured
  `TB_REMEDIATION_*_URLS` or `TB_REMEDIATION_*_DIRS` targets. The
  `bridge_remediation_dispatch_total{target,status}` legend should show
  `success`; `skipped` indicates hooks are unset and
  `persist_failed`/`request_failed` signal spool or HTTP issues that require
  follow-up. The dispatch log endpoint mirrors these statuses so paging/governance
  systems can audit delivery without scraping Prometheus.
- Page the relayer on `playbook="none"` actions, schedule incentive throttles
  when `playbook="incentive-throttle"`, and escalate to governance on
  `playbook="governance-escalation"`. The embedded `response_sequence`
  enumerates these steps explicitly and links back to the liquidity runbook
  anchor for cross-checking.
- If the HTTP hook is unreachable the aggregator logs a WARN, the dispatch
  counter increments `request_failed` or `status_failed`, and the failed
  attempt appears in `/remediation/bridge/dispatches`; remediate the endpoint
  and the next anomaly will be retried automatically.
