# Monitoring

Dashboards are auto-generated from metric definitions in `node/src/telemetry.rs`.

```bash
npm ci --prefix monitoring
make -C monitoring lint   # regenerates grafana/dashboard.json
```

The generator parses Prometheus metric registrations and emits a Grafana dashboard
where each metric appears as a timeseries panel. Customize the output by editing
`monitoring/tools/gen_dashboard.py` and re-running the lint target.
