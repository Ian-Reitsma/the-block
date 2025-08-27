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
- `storage_chunk_size_bytes` – distribution of chunk sizes written during uploads
- `storage_put_chunk_seconds` – time taken to store individual chunks
- `storage_provider_rtt_ms` – observed storage provider round-trip time
- `storage_provider_loss_rate` – observed storage provider loss rate
- `storage_initial_chunk_size` / `storage_final_chunk_size` – first and last chunk sizes per object
- `storage_put_eta_seconds` – estimated total upload time for the current object
- `settle_applied_total` – receipts successfully debited and credited
- `settle_failed_total{reason}` – settlement failures by reason
- `settle_mode_change_total{to}` – settlement mode transitions
- `param_change_pending{key}` – governance parameter changes queued for activation
- `param_change_active{key}` – current active governance parameter values
- `synthetic_convergence_seconds` – end-to-end probe duration emitted by scripts/synthetic.sh
- `synthetic_success_total` – successful synthetic runs
- `synthetic_fail_total{step}` – probe failures by step

For a full list of counters, see `src/telemetry.rs`.
