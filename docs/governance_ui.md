# Governance Web UI

`gov-ui` serves a minimal web interface for governance proposals.
Run with `cargo run -p gov-ui` then open `http://localhost:8080`.
The landing page lists proposals and current vote counts with
accessible, dark‑mode styling. A simple form allows operators to cast
votes via POST without touching the CLI. The server persists votes back
to `examples/governance/proposals.db`.

This prototype focuses on transparency: proposal activation status and
per‑house vote metrics are visible at a glance, and the layout is screen
reader friendly.
