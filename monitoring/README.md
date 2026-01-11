# Monitoring
Guidance aligns with the dependency-sovereignty pivot; runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced.

Dashboards are generated from `metrics.json`. The native and Docker scripts
run `monitoring/tools/render_foundation_dashboard.py` in a loop, producing
`monitoring/output/index.html` that summarises the latest telemetry snapshot.

```bash
npm ci --prefix monitoring
make -C monitoring lint
python monitoring/tools/render_foundation_dashboard.py http://localhost:9898/metrics
```

`make -C monitoring lint` still rebuilds `grafana/dashboard.json` via the Rust
build script so historical Grafana dashboards remain reproducible, but the
first-party viewer consumes the JSON schema directly. Custom overrides in
`dashboard_overrides.json` are merged during generation.

Use `make dashboard` at the repository root to regenerate the dashboard schema
manually. Edits to `metrics.json` or the overrides file automatically trigger
regeneration during builds.

Wrapper metrics appear in the generated dashboards under the **Other** row. The
panels plot `runtime_backend_info`, `transport_provider_info`,
`transport_provider_connect_total`, `coding_algorithm_info`,
`codec_*`, `crypto_*`, and `dependency_policy_violation*` so operators can trace
backend failovers or policy drift alongside the rest of the telemetry feed.
Runtime health panels also chart reactor metrics such as
`runtime_read_without_ready_total` and `runtime_write_without_ready_total` to
flag missed readiness events after kqueue/epoll changes. Fee-lane panels track
`fee_floor_current` plus per-lane `fee_floor_warning_total` /
`fee_floor_override_total` to surface congestion pricing shifts, while P2P
panels use `peer_request_total` and `p2p_request_limit_hits_total` as a proxy
for chain-sync pressure until dedicated chain-sync counters land.
When investigating supply-chain issues, fetch the same data from the
aggregator with:

```bash
contract-cli system dependencies --aggregator http://aggregator.block:9000
```

The CLI sorts wrapper metrics per node and mirrors the `/wrappers` endpoint
used by the dashboards, which makes it easy to paste snapshots into incident
reviews or ticket updates.
