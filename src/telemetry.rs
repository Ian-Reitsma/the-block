use once_cell::sync::Lazy;
use prometheus::{Encoder, IntCounter, IntGauge, Registry, TextEncoder};

pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

pub static MEMPOOL_SIZE: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new("mempool_size", "Current mempool size").unwrap();
    REGISTRY.register(Box::new(g.clone())).unwrap();
    g
});

pub static EVICTIONS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("evictions_total", "Total mempool evictions").unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub static FEE_FLOOR_REJECT_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "fee_floor_reject_total",
        "Transactions rejected for low fee",
    )
    .unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub static DUP_TX_REJECT_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("dup_tx_reject_total", "Transactions rejected as duplicate").unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub static TX_ADMITTED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("tx_admitted_total", "Total admitted transactions").unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub static TX_REJECTED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new("tx_rejected_total", "Total rejected transactions").unwrap();
    REGISTRY.register(Box::new(c.clone())).unwrap();
    c
});

pub fn gather() -> String {
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    let metrics = REGISTRY.gather();
    encoder.encode(&metrics, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap_or_default()
}
