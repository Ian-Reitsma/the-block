# Light Client Incentives

Light clients that relay validity proofs earn micro-rebates in CT tokens. Each verified proof delivery is tracked by the node's `ProofTracker` and aggregated into the next block's coinbase.

## Rebate Accounting

Rebates are deducted from the subsidy pool and credited to the relayer once the block including the proof is finalized. Double-claiming is prevented by zeroing the tracker after each claim.

Telemetry counters `proof_rebates_claimed_total` and `proof_rebates_amount_total` expose the number of claims and total CT awarded.

Governance may tune the rebate rate via the `proof_rebate_rate` parameter. Mobile users can inspect their current balance with `light-client rebate-status`.
