# Monitoring

Dashboards are generated from metric definitions in `node/src/telemetry.rs`.

```bash
npm ci --prefix monitoring
make -C monitoring lint
```

`make -C monitoring lint` runs the full generator pipeline:

1. `tools/gen_schema.py` parses metric registrations and updates `metrics.json`, carrying forward removed metrics with `"deprecated": true`.
2. `tools/gen_dashboard.py` converts the schema into `grafana/dashboard.json` panels.
3. `npx jsonnet-lint` verifies the resulting dashboard.

Edit `metrics.json` to tweak titles, labels, or deprecations. All JSON outputs are formatted with stable ordering for diff-friendly reviews.
