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
double counted. Debug assertions in the shared helper and a new uneven-price
regression keep the split anchored even when oracle prices produce non-zero
remainders, so dashboards never drift from the configured budgets. The expanded
schema also surfaces `clearing_price_usd_micros`, `delivery_channel`, and the
RangeBoost `mesh_payload_digest`, letting dashboards flag mesh deliveries without
shelling out to RPC inspectors.

The static dashboard now calls out the peer-level gauges emitted by the explorer
pipeline—`explorer_block_payout_ad_usd_total`,
`explorer_block_payout_ad_settlement_count`, and the CT/IT oracle snapshots—so
operators can cross-check USD spend and conversion inputs without pivoting to
Grafana. The readiness card now renders both the archived and live marketplace
oracles (`ad_readiness_market_{ct,it}_price_usd_micros`), the settlement totals,
and the `utilization` map returned by `ad_market.readiness`, showing
mean/min/max cohort utilisation plus per-cohort targets, observed ppm, deltas,
and price-per-MiB inputs that informed the latest settlements.

The treasury section gained a **Dual-Token Disbursements** timeline sourced from
the new `Block::treasury_events` payload. Each executed disbursement displays the
beneficiary, token, USD amount, execution height, and the originating
transaction hash so operators can audit payouts alongside the settlement cards.
Dashboards colourise events whose cohorts triggered the
`AdReadinessUtilizationDelta` alert, tying treasury releases back to the
readiness deltas emitted by the metrics aggregator and confirming governance
flags are aligned across runtime, explorer, and telemetry surfaces.

The SPA now includes attestation and pacing cards. Selection receipts display the
latest `ad_selection_attestation_total{kind,result}` breakdown with tooltips
linking to the SNARK circuit identifiers, and the campaign panel renders
`ad_budget_progress`, the live shadow price, and the κ gradient so operators can
confirm the optimal-control pacing stays within governance bounds. Cohort tiles
overlay the PI controller state (`ad_price_pi_error`, `ad_price_pi_integral`,
`ad_price_pi_forgetting`) alongside the latest `ad_resource_floor_component_usd`
values so damping, saturation, badge scoping, and floor composition remain
auditable from the inline dashboard without opening Grafana. `SettlementBreakdown`
payloads now surface the composite `resource_floor_breakdown` directly in the
inline cards, so operators can verify the bandwidth, verifier, and host
contributions that cleared the floor without opening the RPC inspector. The
selection receipt modal mirrors the new structure, showing per-component USD
contributions and the qualified-impressions amortization factor the wallet
proved in its attestation.

Privacy budgets and uplift diagnostics sit alongside the pacing card. The badge
family table colours entries whose `ad_privacy_budget_total{result="cooling"|"revoked"}`
counters move, while inline gauges render the remaining `(ε, δ)` allowance per
family. The uplift panel graphs `ad_uplift_propensity{sample}` and
`ad_uplift_lift_ppm{impressions}` so operators can spot calibration drift without
leaving the dashboard.

Grafana gained a dedicated **Advertising** row to complement the inline cards.
Panels chart five-minute deltas of `ad_selection_attestation_total` by kind and
reason, the SNARK verification latency histogram
`ad_selection_proof_verify_seconds{circuit}`, commitment sizes via
`ad_selection_attestation_commitment_bytes{kind}`, and the campaign pacing trio:
`ad_budget_progress{campaign}`, `ad_budget_shadow_price{campaign}`, and
`ad_budget_kappa_gradient{campaign,...}`. A companion panel breaks out the floor
components using `ad_resource_floor_component_usd{component}` so bandwidth,
verifier, and host costs are obvious when bids clear near the floor. Alert
annotations surface directly on the panels when
`SelectionProofSnarkFallback`, `SelectionProofRejectionSpike`,
`AdBudgetProgressFlat`, or `AdResourceFloorVerifierDrift` fire, keeping
proof-mix regressions, stalled pacing, and verifier amortisation anomalies
visible without digging through PromQL. The row reuses the in-house panel
builders introduced for the explorer/treasury sections, so no third-party
templates or SDKs were required.

Grafana’s pacing row now also charts the broker deltas derived from
`ad_budget_summary_value{metric}` and the new `BudgetBrokerPacingDelta`
aggregate. Operators can correlate the JSON pacing feed with the Prometheus
gauges—`mean_kappa`, `epoch_spend_total_usd`, and `dual_price_max`—to confirm
partial snapshot streams merge deterministically before deltas are exported to
the dashboard.
