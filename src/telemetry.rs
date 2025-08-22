use blake3;
use once_cell::sync::Lazy;
use prometheus::{
    Encoder, GaugeVec, Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Opts, Registry, TextEncoder,
};
use pyo3::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

pub static MEMPOOL_SIZE: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new("mempool_size", "Current mempool size")
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static SNAPSHOT_INTERVAL: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new("snapshot_interval", "Snapshot interval in blocks")
        .unwrap_or_else(|e| panic!("gauge snapshot interval: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry snapshot interval: {e}"));
    g
});

pub static SNAPSHOT_DURATION_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "snapshot_duration_seconds",
        "Snapshot operation duration",
    );
    let h = Histogram::with_opts(opts).unwrap_or_else(|e| panic!("histogram snapshot duration: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry snapshot duration: {e}"));
    h
});

pub static SNAPSHOT_FAIL_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "snapshot_fail_total",
        "Total snapshot operation failures",
    )
    .unwrap_or_else(|e| panic!("counter snapshot fail: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry snapshot fail: {e}"));
    c
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

pub static BANNED_PEERS_TOTAL: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new("banned_peers_total", "Total peers currently banned")
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static BANNED_PEER_EXPIRATION: Lazy<IntGaugeVec> = Lazy::new(|| {
    let g = IntGaugeVec::new(
        Opts::new(
            "banned_peer_expiration_seconds",
            "Expiration timestamp for active peer bans",
        ),
        &["peer"],
    )
    .unwrap_or_else(|e| panic!("gauge vec: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static COURIER_FLUSH_ATTEMPT_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "courier_flush_attempt_total",
        "Total courier receipt flush attempts",
    )
    .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static COURIER_FLUSH_FAILURE_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "courier_flush_failure_total",
        "Failed courier receipt flush attempts",
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

pub static PEER_ERROR_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        prometheus::Opts::new("peer_error_total", "Total peer errors grouped by code"),
        &["code"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static RPC_CLIENT_ERROR_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        prometheus::Opts::new(
            "rpc_client_error_total",
            "Total RPC client errors grouped by code",
        ),
        &["code"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static RPC_TOKENS: Lazy<GaugeVec> = Lazy::new(|| {
    let g = GaugeVec::new(
        Opts::new(
            "rpc_tokens_available",
            "Current RPC rate limiter tokens per client",
        ),
        &["client"],
    )
    .unwrap_or_else(|e| panic!("gauge vec: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static RPC_BANS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("rpc_bans_total", "Total RPC bans issued")
        .unwrap_or_else(|e| panic!("counter: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static BADGE_ACTIVE: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new("badge_active", "Whether a service badge is active (1/0)")
        .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
});

pub static BADGE_LAST_CHANGE_SECONDS: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new(
        "badge_last_change_seconds",
        "Unix timestamp of the last badge mint/burn",
    )
    .unwrap_or_else(|e| panic!("gauge: {e}"));
    REGISTRY
        .register(Box::new(g.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    g
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

pub const LOG_FIELDS: &[&str] = &[
    "subsystem",
    "op",
    "sender",
    "nonce",
    "reason",
    "code",
    "fpb",
];

pub static LOG_EMIT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        prometheus::Opts::new("log_emit_total", "Total emitted log events"),
        &["subsystem"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static LOG_DROP_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let c = IntCounterVec::new(
        prometheus::Opts::new("log_drop_total", "Logs dropped due to rate limiting"),
        &["subsystem"],
    )
    .unwrap_or_else(|e| panic!("counter_vec: {e}"));
    REGISTRY
        .register(Box::new(c.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    c
});

pub static LOG_SIZE_BYTES: Lazy<Histogram> = Lazy::new(|| {
    let opts = HistogramOpts::new("log_size_bytes", "Size of serialized log events in bytes")
        .buckets(
            prometheus::exponential_buckets(64.0, 2.0, 8)
                .unwrap_or_else(|e| panic!("histogram buckets: {e}")),
        );
    let h = Histogram::with_opts(opts).unwrap_or_else(|e| panic!("histogram: {e}"));
    REGISTRY
        .register(Box::new(h.clone()))
        .unwrap_or_else(|e| panic!("registry: {e}"));
    h
});

static LOG_SEC: AtomicU64 = AtomicU64::new(0);
static LOG_COUNT: AtomicU64 = AtomicU64::new(0);

/// Maximum log events per second before sampling kicks in.
pub const LOG_LIMIT: u64 = 100;
/// After `LOG_LIMIT` is exceeded, emit one in every `LOG_SAMPLE_STRIDE` events.
pub const LOG_SAMPLE_STRIDE: u64 = 100;

pub fn should_log(subsystem: &str) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let last = LOG_SEC.load(Ordering::Relaxed);
    if now != last {
        LOG_SEC.store(now, Ordering::Relaxed);
        LOG_COUNT.store(0, Ordering::Relaxed);
    }
    let count = LOG_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if count <= LOG_LIMIT || count % LOG_SAMPLE_STRIDE == 0 {
        LOG_EMIT_TOTAL.with_label_values(&[subsystem]).inc();
        true
    } else {
        LOG_DROP_TOTAL.with_label_values(&[subsystem]).inc();
        false
    }
}

pub fn observe_log_size(bytes: usize) {
    LOG_SIZE_BYTES.observe(bytes as f64);
}

#[doc(hidden)]
pub fn reset_log_counters() {
    LOG_SEC.store(0, Ordering::Relaxed);
    LOG_COUNT.store(0, Ordering::Relaxed);
    for sub in ["mempool", "storage", "p2p", "compute"] {
        LOG_EMIT_TOTAL.with_label_values(&[sub]).reset();
        LOG_DROP_TOTAL.with_label_values(&[sub]).reset();
    }
}

#[pyfunction]
pub fn redact_at_rest(dir: &str, hours: u64, hash: bool) -> PyResult<()> {
    use std::fs;
    use std::time::Duration;

    let cutoff = SystemTime::now() - Duration::from_secs(hours * 3600);
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let meta = entry.metadata()?;
            if let Ok(modified) = meta.modified() {
                if modified < cutoff {
                    if hash {
                        let data = fs::read(&path)?;
                        let digest = blake3::hash(&data).to_hex().to_string();
                        fs::write(&path, digest)?;
                    } else {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
    }
    Ok(())
}

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
        LOG_EMIT_TOTAL.with_label_values(&["mempool"]),
        LOG_EMIT_TOTAL.with_label_values(&["storage"]),
        LOG_EMIT_TOTAL.with_label_values(&["p2p"]),
        LOG_EMIT_TOTAL.with_label_values(&["compute"]),
        LOG_DROP_TOTAL.with_label_values(&["mempool"]),
        LOG_DROP_TOTAL.with_label_values(&["storage"]),
        LOG_DROP_TOTAL.with_label_values(&["p2p"]),
        LOG_DROP_TOTAL.with_label_values(&["compute"]),
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
