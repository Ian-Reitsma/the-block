# Monitoring

Dashboards are generated from `metrics.json`.

```bash
npm ci --prefix monitoring
make -C monitoring lint
```

`make -C monitoring lint` rebuilds `grafana/dashboard.json` via the Rust build
script and validates the result with `jq` and `jsonnet-lint`. Custom overrides
in `dashboard_overrides.json` are merged during generation.

Use `make dashboard` at the repository root to regenerate the dashboard manually.
Edits to `metrics.json` or the overrides file automatically trigger
regeneration during builds.

Wrapper metrics appear in the generated dashboards under the **Other** row. The
panels plot `runtime_backend_info`, `transport_provider_info`,
`transport_provider_connect_total`, `coding_algorithm_info`,
`codec_*`, `crypto_*`, and `dependency_policy_violation*` so operators can trace
backend failovers or policy drift alongside the rest of the telemetry feed.
When investigating supply-chain issues, fetch the same data from the
aggregator with:

```bash
contract-cli system dependencies --aggregator http://aggregator.block:9000
```

The CLI sorts wrapper metrics per node and mirrors the `/wrappers` endpoint
used by the dashboards, which makes it easy to paste snapshots into incident
reviews or ticket updates.
