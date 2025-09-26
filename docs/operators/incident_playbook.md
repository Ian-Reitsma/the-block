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
