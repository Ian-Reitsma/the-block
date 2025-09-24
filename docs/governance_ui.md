# Governance Web UI
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

`gov-ui` serves a minimal web interface for governance proposals.
Run with `cargo run -p gov-ui` then open `http://localhost:8080`.
The landing page now streams subsidy-parameter proposals and
changes directly from a running node via JSON‑RPC. Proposals
show the key and pending value. Operators can
cast votes for these items using the form at the bottom of the
page; votes are relayed to the node through the `inflation.params` RPC.

This prototype focuses on transparency: proposal activation status and
per‑house vote metrics are visible at a glance, and the layout is screen
reader friendly.