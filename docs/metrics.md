# Telemetry and Prometheus Metrics

The node exposes internal counters via a minimal HTTP exporter when compiled
with the `telemetry` feature. Start a node with the `--metrics-addr` flag and
visit the `/metrics` endpoint to scrape metrics in the Prometheus text format.

```bash
$ cargo run --bin node --features telemetry -- run --metrics-addr 127.0.0.1:9100
```

Sample output:

```bash
$ curl -s http://127.0.0.1:9100/metrics | head -n 5
# HELP tx_submitted_total Total submitted transactions
# TYPE tx_submitted_total counter
tx_submitted_total 0
# HELP block_mined_total Total mined blocks
# TYPE block_mined_total counter
```

The exporter currently tracks:

- `tx_submitted_total` – transactions submitted to the mempool
- `tx_rejected_total{reason}` – transactions rejected with a labeled reason
- `block_mined_total` – blocks successfully mined
- `mempool_size` – gauge of current mempool size

For a full list of counters, see `src/telemetry.rs`.
