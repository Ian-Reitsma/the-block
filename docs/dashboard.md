# Dashboard
> **Review (2025-09-25):** Synced Dashboard guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The node exposes a lightweight dashboard at `/dashboard` on the RPC HTTP port. The
page renders a small SPA that displays mempool depth, price bands, subsidy
counters, read-denial statistics (`read_denied_total{reason}`), LocalNet
statistics, and the latest ad-readiness snapshot (ready flag, unique viewers,
host/provider counts, configured minimums, and skip reasons). Operators can
point a browser at `http://<node>:<rpc_port>/dashboard` to view the metrics.

The dashboard is served as a static bundle from the node binary, requiring no additional assets at runtime.

The “Block Payouts” row on the Grafana bundle and the inline ad-settlement cards
now consume `SettlementBreakdown` directly. With the liquidity CT conversion
respecting `liquidity_split_ct_ppm`, the CT and IT totals rendered in the
dashboard match the USD amounts and oracle snapshot captured during settlement,
removing the temporary over-reporting that appeared while the IT share was
double counted.

The static dashboard now calls out the peer-level gauges emitted by the explorer
pipeline—`explorer_block_payout_ad_usd_total`,
`explorer_block_payout_ad_settlement_count`, and the CT/IT oracle snapshots—so
operators can cross-check USD spend and conversion inputs without pivoting to
Grafana. The readiness card links to the new `utilization` map returned by
`ad_market.readiness`, showing mean/min/max cohort utilisation alongside the per-
cohort targets and prices that informed the latest settlements.
