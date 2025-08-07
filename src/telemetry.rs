use once_cell::sync::Lazy;
use prometheus::{Encoder, IntCounter, IntGauge, Registry, TextEncoder};

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

pub static TX_REJECTED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("tx_rejected_total", "Total rejected transactions")
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

pub fn gather() -> String {
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    let metrics = REGISTRY.gather();
    encoder
        .encode(&metrics, &mut buffer)
        .unwrap_or_else(|e| panic!("encode: {e}"));
    String::from_utf8(buffer).unwrap_or_default()
}
