# Incident playbook

## Convergence lag
- Run `just probe:tip` and inspect `gossip_convergence_seconds`.
- Inspect peers via `logs` and ensure feature bits match.
- Gather `just support:bundle` and attach to ticket.

## High consumer fees
- Check proposals adjusting `ConsumerFeeComfortP90Microunits`.
- Review `mempool` pressure and pending activations.
- Consider proposing a higher comfort threshold.

## Industrial stalls
- Inspect `admission_rejected_total{reason=*}` and `record_available_shards`.
- Adjust `IndustrialAdmissionMinCapacity` or quotas.

## Data corruption
- Watch `price_board_load_total{result="corrupt"}`; node auto-recovers.
- If repeated, replace disk after taking a support bundle.
