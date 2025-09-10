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
