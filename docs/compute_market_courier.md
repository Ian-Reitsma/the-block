# Compute-Market Courier and Retry Logic

The compute market supports a store-and-forward "courier" mode where contributors shuttle data bundles across intermittent links. Each bundle generates a durable receipt that is retried with exponential backoff until an upstream worker acknowledges it. This guide explains the receipt format, storage layout, retry schedule, and operational tooling.

## 1. Receipt Structure

`node/src/compute_market/courier.rs` defines `CourierReceipt`:

- `id` – random 64-bit identifier for the bundle.
- `bundle_hash` – BLAKE3 hex digest of the payload ensuring integrity.
- `sender` – account string of the forwarding node.
- `timestamp` – UNIX seconds when `send` was called.
- `acknowledged` – set to `true` once the bundle is confirmed upstream.

Receipts are bincode-serialised and persisted in a sled tree named `courier`.

## 2. Sending Bundles

`CourierStore::send` takes raw bytes and a sender ID. It hashes the bundle, allocates an ID via `OsRng`, writes the receipt to disk, and returns it to the caller for logging or display:

```rust
let store = CourierStore::open("/var/lib/theblock/courier");
let receipt = store.send(&bundle, "alice");
```

The store path is fully configurable by the caller; production deployments usually point to a dedicated volume to survive restarts.

## 3. Flush and Exponential Backoff

`flush` iterates over unacknowledged receipts and invokes a user-supplied forwarding closure `forward(&CourierReceipt) -> bool`. The closure should attempt to deliver the bundle and return `true` on success.

For each receipt:

1. Start with `attempt = 0` and `delay = 100ms`.
2. Call `forward`. On success, mark `acknowledged = true` and update the sled record.
3. On failure, increment `attempt`, double `delay`, and `sleep(delay)`.
4. Give up after five attempts (`attempt >= 5`). The receipt remains on disk for the next `flush`.

This geometric backoff smooths network hiccups while bounding worst-case latency.

Telemetry features (`--features telemetry`) increment `COURIER_FLUSH_ATTEMPT_TOTAL` and `COURIER_FLUSH_FAILURE_TOTAL` counters with optional `tracing` logs.

## 4. Operational Guidelines

- **Flush Cadence** – Operators typically schedule `flush` every few seconds or on connectivity changes. A background task calling `flush` with a channel to the worker is common.
- **Crash Recovery** – Because receipts live in sled, the node can crash or reboot without losing pending bundles. `send` is idempotent; calling it twice with the same bundle creates two receipts with different IDs.
- **Manual Inspection** – A CLI wrapper may expose `courier list` and `courier resend <id>` commands for debugging.
- **Backpressure** – If the forwarding endpoint is down, receipts will accumulate. Monitor disk usage and `COURIER_FLUSH_FAILURE_TOTAL` to alert operators.

## 5. Example Forwarder

```rust
let acknowledged = store.flush(|rec| {
    // Replace with real network send
    network_send(rec.bundle_hash.clone())
});
println!("acknowledged {acknowledged} bundles");
```

`flush` returns the count of newly acknowledged receipts, allowing callers to report progress.

## 6. Related References

- Price board and compute market overview: `docs/compute_market.md`.
- Telemetry metrics: `docs/metrics.md` and `README` "Telemetry & Metrics" section.
- For regression testing, run `cargo nextest run --features telemetry compute_market::courier_retry_updates_metrics`.

Keep this document updated when modifying courier semantics so operators can reason about persistence, retries, and monitoring.
