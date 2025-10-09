# AI Diagnostics
> **Review (2025-09-25):** Synced AI Diagnostics guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This document outlines the experimental AI-assisted diagnostics pipeline.
Models analyse telemetry streams to flag anomalies and suggest configuration
tweaks. Training happens off-line using datasets exported via the node
telemetry module, respecting privacy policies.

Operators can submit anomaly labels back to the network which are aggregated
via runtime telemetry counters. Governance may toggle the diagnostics feature via
`ai_diagnostics_enabled` parameter.
