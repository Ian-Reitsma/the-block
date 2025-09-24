# Network Fee Rebates
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Peers with high uptime earn monthly rebate vouchers payable in IT. Nodes track uptime in `net/uptime.rs` and persist per‑peer totals in `peer_metrics_store`. Operators may query eligibility via the `peer.rebate_status` RPC. Eligible peers claim vouchers with `peer.rebate_claim`, which mining nodes include in coinbase transactions.

Governance parameters `rebate_threshold_secs` and `rebate_cap` adjust eligibility and maximum payout per epoch.

Metrics:
- `rebate_claims_total` counts submitted claims.
- `rebate_issued_total` tracks vouchers issued.

Use the CLI `net rebate claim` to redeem vouchers.