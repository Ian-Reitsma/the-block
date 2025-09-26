# Changelog
> **Review (2025-09-25):** Logged wrapper telemetry rollout, codec/crypto consolidation, and refreshed readiness/pillar metrics after verifying wrapper adoption data.

## 2025-09-25 — Wrapper Telemetry & Codec Consolidation

- Instrumented runtime, transport, overlay, storage engine, coding, codec, and crypto wrappers end-to-end. The node now exports `runtime_backend_info`, `transport_provider_connect_total{provider}`, `codec_serialize_fail_total{profile}`, and crypto suite counters, and the metrics aggregator surfaces `/wrappers` summaries with regression coverage.
- Introduced the first-party `codec` crate with named profiles (`transaction`, `gossip`, `cbor::default`), serde bridging macros, telemetry hooks, and corruption-focused round-trip tests. CLI, explorer, gossip relay, storage manifests, and transaction bindings route through the wrapper for consistent errors.
- Landed the `crypto_suite` crate providing trait-based signatures, hashing, key derivation, Groth16 helpers, domain-tag utilities, and benchmarking harnesses; the `crypto` facade re-exports the suite so legacy imports compile during migration.
- Regenerated Grafana dashboards and metrics schemas to visualise wrapper health, backend selections, dependency drift gauges, and codec/crypto error rates across operator, dev, telemetry, and root dashboards.
- Added `contract-cli system dependencies` to fetch wrapper snapshots, wired the dependency registry to emit `dependency_policy_violation` gauges, and documented new telemetry in `docs/telemetry.md` plus serialization guardrails in `docs/serialization.md`.
- Re-scored readiness to **98.9/92.0** and synchronized pillar percentages across `README.md`, `AGENTS.md`, `docs/progress.md`, and `docs/roadmap.md` with supporting evidence from wrapper telemetry and dependency dashboards.

## Historical Highlights

- 20cd47e · Added service badge governance records and law-enforcement portal audit trail with explorer timelines.
- e4243c4 · Hardened admin authentication by comparing RPC tokens in constant time to prevent timing leaks.
- [#0000](https://github.com/owner/repo/pull/0000) · Bootstrapped documentation tooling, linting, and examples to keep subsystem specs aligned.