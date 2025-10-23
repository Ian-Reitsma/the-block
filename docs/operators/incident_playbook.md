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
  the action/playbook pair, dispatch outcomes, acknowledgement state, and
  acknowledgement latency alongside `bridge_liquidity_*` asset deltas and the
  annotation-aware response text. The acknowledgement counters chart
  `sum by (action, playbook, target, state)(increase(bridge_remediation_dispatch_ack_total[5m]))`
  so you can confirm paging/governance systems closed the loop, while the latency
  histogram overlays p50/p95 values from
  `bridge_remediation_ack_latency_seconds{playbook,state}` to highlight slow
  closures before the escalation window expires.
- When an action fires, inspect `/remediation/bridge` for the persisted entry
  and `/remediation/bridge/dispatches` for the per-target delivery log. The
  payload now includes `acknowledged_at`, `closed_out_at`, and
  `acknowledgement_notes` whenever the downstream system posts an acknowledgement,
  in addition to the `annotation`, `dashboard_panels`, and `response_sequence`
  that outline the expected operator steps.
- Confirm the JSON payload has been dispatched to the configured
  `TB_REMEDIATION_*_URLS` or `TB_REMEDIATION_*_DIRS` targets. The
  `bridge_remediation_dispatch_total{target,status}` legend should show
  `success`; `skipped` indicates hooks are unset and
  `persist_failed`/`request_failed` signal spool or HTTP issues that require
  follow-up. Pair those statuses with the acknowledgement counter:
  `pending` means the hook has yet to confirm, `acknowledged` indicates the
  pager/governance queue accepted the playbook, and `closed` marks completion.
  The dispatch log endpoint mirrors both sets of fields so downstream systems can
  audit delivery without scraping Prometheus.
- Page the relayer on `playbook="none"` actions, schedule incentive throttles
  when `playbook="incentive-throttle"`, and escalate to governance on
  `playbook="governance-escalation"`. The embedded `response_sequence`
  enumerates these steps explicitly and links back to the liquidity runbook
  anchor for cross-checking.
- Use `contract remediation bridge --aggregator http://<host>:9000 --limit 10`
  to print the most recent actions, retry counts, follow-up notes, and dispatch
  history directly from the aggregator. The CLI mirrors the JSON endpoints but
  keeps everything inside the first-party tooling for incident review.
- Adjust acknowledgement policy on a per-playbook basis with
  `TB_REMEDIATION_ACK_RETRY_SECS`, `TB_REMEDIATION_ACK_ESCALATE_SECS`, and
  `TB_REMEDIATION_ACK_MAX_RETRIES` for the defaults, or suffix overrides such as
  `TB_REMEDIATION_ACK_RETRY_SECS_GOVERNANCE_ESCALATION` when governance hooks
  need longer buffers. The aggregator will respect the overrides immediately and
  record completion latency against the histogram panel for audit.
- If the HTTP hook is unreachable the aggregator logs a WARN, the dispatch
  counter increments `request_failed` or `status_failed`, and the failed
  attempt appears in `/remediation/bridge/dispatches`; remediate the endpoint
  and the next anomaly will be retried automatically. A hook that returns an
  invalid acknowledgement increments the `state="invalid"` series so you can
  escalate malformed responses.
