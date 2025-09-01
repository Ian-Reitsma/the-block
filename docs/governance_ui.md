# Governance Web UI

`gov-ui` serves a minimal web interface for governance proposals.
Run with `cargo run -p gov-ui` then open `http://localhost:8080`.
The landing page now streams credit‑issuance proposals and parameter
changes directly from a running node via JSON‑RPC. Credit proposals
display the provider and amount under vote, while parameter proposals
such as `ReadPoolSeed` show the key and pending value. Operators can
cast votes for credit‑issuance items using the form at the bottom of the
page; votes are relayed to the node through the `gov_credit_vote` RPC.

This prototype focuses on transparency: proposal activation status and
per‑house vote metrics are visible at a glance, and the layout is screen
reader friendly.
