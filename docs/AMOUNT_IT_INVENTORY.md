# amount_it / balance_it Inventory

## Summary
- `rg -c "amount_it"` now reports matches only in documentation/backlog trackers and historical references (governance/CLI structs were migrated).
- `rg -c "balance_it"` now lands only in legacy audit JSON (optional fields) and docs; runtime state keeps a single provider balance.
- Remaining work is confined to **documentation cleanup**, **dashboard migrations**, and **post-mortem summaries** that still mention the legacy fields.

## Key areas to scope

| Area | Notable files / counts | Notes |
| --- | --- | --- |
| Governance models | ✅ Complete | Single-field structs + versioned codec |
| Node RPC / receivers | ✅ Complete | RPC payloads return `amount`/`treasury_balance` only |
| CLI / Explorer | ✅ Explorer DB migration script (`explorer-migrate-treasury`) + runbook are in-tree |
| Metrics / telemetry | ✅ Runtime metrics/dashboards reference `treasury_balance` only |
| Tests & docs | ⚠️ Remaining markdown references (checklist, audit docs, runbooks) still describe dual-field payloads |

## Next steps (Phase 1 continuation)
1. Finish the documentation sweeps (README tables, runbooks, audits, schema references) so user-facing guidance never mentions `amount_it`.
2. Rebuild Grafana dashboards + Prometheus rules to use the single `treasury_balance` gauge, then attach screenshots per AGENTS.
3. Mirror any future TODOs in §15 of `AGENTS.md` so the backlog stays visible.

This inventory document will be updated as we mark files off during each phase.
