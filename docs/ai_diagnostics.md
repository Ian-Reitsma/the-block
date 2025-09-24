# AI Diagnostics
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

This document outlines the experimental AI-assisted diagnostics pipeline.
Models analyse telemetry streams to flag anomalies and suggest configuration
tweaks. Training happens off-line using datasets exported via the node
telemetry module, respecting privacy policies.

Operators can submit anomaly labels back to the network which are aggregated
via Prometheus counters. Governance may toggle the diagnostics feature via
`ai_diagnostics_enabled` parameter.