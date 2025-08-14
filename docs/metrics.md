# Metrics

The node exposes Prometheus metrics when started with `--metrics-addr`:

```bash
cargo run --bin node -- run --metrics-addr 127.0.0.1:9001
curl -s http://localhost:9001/metrics
```

Example scrape output:

```text
# HELP tx_submitted_total Total submitted transactions
# TYPE tx_submitted_total counter
tx_submitted_total 1
# HELP tx_rejected_total Total rejected transactions
# TYPE tx_rejected_total counter
tx_rejected_total{reason="duplicate"} 1
# HELP block_mined_total Total mined blocks
# TYPE block_mined_total counter
block_mined_total 1
```

Metric descriptions:

| Metric | Meaning |
|--------|---------|
| `tx_submitted_total` | Count of all transactions submitted for admission |
| `tx_rejected_total{reason}` | Labelled count of rejected transactions with the provided reason |
| `block_mined_total` | Number of blocks successfully mined |

Other counters, such as `mempool_size` and `ttl_drop_total`, remain available
for deeper inspection. The endpoint returns standard Prometheus text suitable
for scraping and alerting.

