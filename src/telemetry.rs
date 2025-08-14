use once_cell::sync::Lazy;
use prometheus::{Encoder, IntCounter, IntCounterVec, IntGauge, Registry, TextEncoder};
use pyo3::prelude::*;

pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

pub static MEMPOOL_SIZE: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new("mempool_size", "Current mempool size")
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static EVICTIONS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("evictions_total", "Total mempool evictions")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static FEE_FLOOR_REJECT_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "fee_floor_reject_total",
        "Transactions rejected for low fee",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static DUP_TX_REJECT_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("dup_tx_reject_total", "Transactions rejected as duplicate")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static TX_ADMITTED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("tx_admitted_total", "Total admitted transactions")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static TX_SUBMITTED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("tx_submitted_total", "Total submitted transactions")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static TX_REJECTED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        prometheus::Opts::new("tx_rejected_total", "Total rejected transactions"),
        &["reason"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static BLOCK_MINED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("block_mined_total", "Total mined blocks")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static TTL_DROP_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "ttl_drop_total",
        "Transactions dropped due to TTL expiration",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static STARTUP_TTL_DROP_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "startup_ttl_drop_total",
        "Expired mempool entries dropped during startup",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static LOCK_POISON_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "lock_poison_total",
        "Lock acquisition failures due to poisoning",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static ORPHAN_SWEEP_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "orphan_sweep_total",
        "Transactions dropped because the sender account is missing",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static INVALID_SELECTOR_REJECT_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "invalid_selector_reject_total",
        "Transactions rejected for invalid fee selector",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static BALANCE_OVERFLOW_REJECT_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "balance_overflow_reject_total",
        "Transactions rejected due to balance overflow",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static DROP_NOT_FOUND_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "drop_not_found_total",
        "drop_transaction failures for missing entries",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub struct Recorder;

impl Recorder {
    pub fn tx_submitted(&self) {
        TX_SUBMITTED_TOTAL.inc();
    }

    pub fn tx_rejected(&self, reason: &str) {
        TX_REJECTED_TOTAL.with_label_values(&[reason]).inc();
    }

    pub fn block_mined(&self) {
        BLOCK_MINED_TOTAL.inc();
    }
}

pub static RECORDER: Recorder = Recorder;

fn gather() -> String {
    // Ensure all metrics are registered even if they haven't been used yet so
    // `gather_metrics` always exposes a stable set of counters.
    let _ = (
        &*MEMPOOL_SIZE,
        &*EVICTIONS_TOTAL,
        &*FEE_FLOOR_REJECT_TOTAL,
        &*DUP_TX_REJECT_TOTAL,
        &*TX_ADMITTED_TOTAL,
        &*TX_SUBMITTED_TOTAL,
        &*TX_REJECTED_TOTAL,
        &*BLOCK_MINED_TOTAL,
        &*TTL_DROP_TOTAL,
        &*STARTUP_TTL_DROP_TOTAL,
        &*LOCK_POISON_TOTAL,
        &*ORPHAN_SWEEP_TOTAL,
        &*INVALID_SELECTOR_REJECT_TOTAL,
        &*BALANCE_OVERFLOW_REJECT_TOTAL,
        &*DROP_NOT_FOUND_TOTAL,
    );

    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    let metrics = REGISTRY.gather();
    encoder
        .encode(&metrics, &mut buffer)
        .unwrap_or_else(|e| panic!("encode: {e}"));
    String::from_utf8(buffer).unwrap_or_default()
}

#[pyfunction]
pub fn gather_metrics() -> PyResult<String> {
    Ok(gather())
}

/// Start a minimal HTTP server that exposes Prometheus metrics.
///
/// The server runs on a background thread and responds to any incoming
/// connection with the current metrics in text format. The bound socket
/// address is returned so callers can discover the chosen port when using
/// an ephemeral one (e.g. `"127.0.0.1:0"`).
///
/// This helper is intentionally lightweight and meant for tests or local
/// demos; production deployments should place a reverse proxy in front of it.
#[pyfunction]
pub fn serve_metrics(addr: &str) -> PyResult<String> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let mut _req = [0u8; 512];
                let _ = stream.read(&mut _req);
                let body = gather_metrics().unwrap_or_else(|e| e.to_string());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        }
    });
    Ok(local.to_string())
}
