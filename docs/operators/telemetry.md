# Telemetry overview

Headline panels show:
- **Probe success rate 10m** – expect ~100%.
- **Convergence p95 (s)** – normal < 3s.
- **Consumer fee p90 vs comfort** – track for fee spikes.
- **Industrial defer ratio 10m** – high values indicate capacity pressure.

To scrape metrics remotely with Prometheus:
```yaml
scrape_configs:
  - job_name: node
    static_configs:
      - targets: ['node-host:9898']
```
Use `scripts/telemetry_sweep.sh` to generate a static `status/index.html` snapshot.
