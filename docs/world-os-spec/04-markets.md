# Markets — Ad/ANN, Storage, Compute, and Settlement Surfaces

## 1. Ad / ANN Market
### Modules & Files
- `crates/ad_market/` — Core library implementing ANN book, bid/ask records, and settlement math. Uses `foundation_serialization` for deterministic encoding.
- `node/src/rpc/ad_market.rs` — JSON-RPC endpoints `ad.market_state`, `ad.submit_bid`, `ad.cancel`, `ad.snapshot`. Exposed via CLI `tb-cli ad ...` and explorer dashboards.
- `cli/src/ad.rs` — CLI commands for registration, bidding, and introspection.
- `monitoring/src/dashboard.rs` — Panels for `ad_total_usd_micros`, ANN settlement counts, and price curves (see `BRIDGE_SETTLEMENT_RESULTS_PANEL_TITLE` for reference).

### State Structures
| Item | Description |
| --- | --- |
| `ad_market::Placement` | Contains ANN embedding hash, reward multiplier, TTL, jurisdiction tags. Stored in sled tree `ad:placements`. |
| `ad_market::Bid` | Price-per-attention delta with CT deposit/backing. Stored in `ad:bids`. |
| `ad_market::Settlement` | Records deliveries and payouts (see `cli/src/explorer.rs` logging). |

### Flow
1. Advertisers submit ANN bids using CLI/RPC. `ad_market` stores bids, calculates EWMA pricing, and reserves CT deposit.
2. Range Boost / LocalNet surfaces request placements; matches resolved against ANN embeddings.
3. Deliveries generate `ad_market::Settlement` records. Treasury debit occurs automatically (CT 95% to provider, 5% to treasury) following the same basis as compute storage flows.
4. RPC `ad.market_state` returns backlog, price bands, and settlement counts; dashboards consume this for the “Markets” page.

## 2. Storage Market (recap)
Covered deeply in `02-service-credits.md`. Key market hooks:
- RPC `storage.upload/challenge/provider_profiles/incentives/repair_*`.
- CLI `tb-cli storage` commands.
- Telemetry `storage_contract_created_total`, `retrieval_success_total`, `retrieval_failure_total`.
These patterns are cloned for the energy vertical.

## 3. Compute Market (recap)
See `02-service-credits.md`. Additional market features:
- `node/src/compute_market/matcher.rs` fairness rotation ensures lane quotas.
- `node/src/compute_market/price_board.rs` drives ANN/ad fairness windows when compute+ad workloads share GPU providers.
- Telemetry histograms `match_loop_latency_seconds{lane}` emitted via `foundation_metrics` and ingested by `metrics-aggregator`.

## 4. Bridge / Settlement Markets
- `bridges/` crate + `node/src/rpc/bridge.rs` implement cross-chain settlement (withdrawals + proof submissions). CLI `tb-cli bridge settlement` uses RPC `bridge.submit_settlement`.
- `monitoring/src/dashboard.rs` includes `bridge_settlement_results_total` panel; keep energy settlements keyed similarly for parity.

## 5. Explorer / Indexer Hooks
- `explorer/src/lib.rs` records provider stats (`provider_stats` table) capturing capacity + reputation. `explorer/src/storage_view.rs` echoes the same data.
- `tools/indexer` processes ANN/ad, storage, and compute receipts for SQLite dashboards. Energy market receipts must conform to this schema (append-only, keyed by provider ID + timestamp) so dashboards render without extra migrations.

## 6. CLI + RPC Checklist for New Markets
When adding the energy market:
1. Mirror `cli/src/storage.rs` command structure for provider registration + challenge/settlement.
2. Add `node/src/rpc/energy_market.rs` (once crate is wired) that exposes `energy.register_provider`, `energy.market_state`, `energy.settle`, `energy.oracle_reading`. Ensure RPC uses canonical JSON helpers from `foundation_serialization::json`.
3. Update `metrics-aggregator` + `monitoring` dashboards with counters similar to `storage`/`compute` panels.
4. Document CLI workflows in `docs/testnet/ENERGY_QUICKSTART.md` (see Step 3 instructions).

Every market must reuse the first-party HTTP + serialization stack (`crates/httpd`, `foundation_serialization`)—never add third-party networking per AGENTS.md.
