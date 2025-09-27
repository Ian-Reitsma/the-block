# Network Partition Recovery
> **Review (2025-09-25):** Synced Network Partition Recovery guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This document describes the procedures for detecting network partitions and reconciling forks once connectivity is restored.

* `net::partition_watch` tracks peer reachability and raises `partition_events_total` when a split is detected.
* Gossip messages carry an optional `partition` marker so downstream peers can avoid diverging histories.
* `consensus::fork_choice` records rollback checkpoints allowing tips to revert gracefully.
* `partition_recover::replay_blocks` replays missing blocks against the local chain and reports progress via `partition_recover_blocks`.
* Persistent records are written through `state::partition_log` for post-mortem analysis.

Operators can monitor `partition_events_total` and `partition_recover_blocks` metrics through Grafana and the `partition_probe` CLI.
