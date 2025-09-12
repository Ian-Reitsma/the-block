use crate::compute_market::courier_store::ReceiptStore;
use crate::compute_market::receipt::Receipt;
use crate::transaction::FeeLane;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Bid {
    pub job_id: String,
    pub buyer: String,
    pub price: u64,
    pub lane: FeeLane,
}

#[derive(Clone)]
pub struct Ask {
    pub job_id: String,
    pub provider: String,
    pub price: u64,
    pub lane: FeeLane,
}

static BIDS: Lazy<Mutex<Vec<Bid>>> = Lazy::new(|| Mutex::new(Vec::new()));
static ASKS: Lazy<Mutex<Vec<Ask>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Replace the current order book with the provided bids and asks.
pub fn seed_orders(bids: Vec<Bid>, asks: Vec<Ask>) {
    *BIDS.lock().unwrap_or_else(|e| e.into_inner()) = bids;
    *ASKS.lock().unwrap_or_else(|e| e.into_inner()) = asks;
}

fn snapshot() -> (Vec<Bid>, Vec<Ask>) {
    (
        BIDS.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        ASKS.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    )
}

fn stable_match(mut bids: Vec<Bid>, mut asks: Vec<Ask>) -> Vec<(Bid, Ask)> {
    bids.sort_by(|a, b| b.price.cmp(&a.price));
    asks.sort_by(|a, b| a.price.cmp(&b.price));
    let mut pairs = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    while i < bids.len() && j < asks.len() {
        let bid = &bids[i];
        let ask = &asks[j];
        if bid.price >= ask.price && bid.lane == ask.lane {
            pairs.push((bid.clone(), ask.clone()));
            i += 1;
            j += 1;
        } else {
            if bid.price < ask.price {
                j += 1;
            } else {
                i += 1;
            }
        }
    }
    pairs
}

const MATCH_INTERVAL: Duration = Duration::from_millis(10);

/// Continuously attempt to match bids and asks, emitting receipts.
pub async fn match_loop(store: ReceiptStore, dry_run: bool, stop: CancellationToken) {
    while !stop.is_cancelled() {
        let _start = std::time::Instant::now();
        let (bids, asks) = snapshot();
        for (bid, ask) in stable_match(bids, asks) {
            let receipt = Receipt::new(
                bid.job_id.clone(),
                bid.buyer.clone(),
                ask.provider.clone(),
                ask.price,
                1,
                dry_run,
            );
            match store.try_insert(&receipt) {
                Ok(true) => {
                    #[cfg(feature = "telemetry")]
                    crate::telemetry::MATCHES_TOTAL
                        .with_label_values(&[if dry_run { "true" } else { "false" }])
                        .inc();
                    #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                    tracing::info!(job = %receipt.job_id, buyer = %receipt.buyer, provider = %receipt.provider, price = receipt.quote_price, dry = receipt.dry_run, "match");
                }
                Ok(false) => {}
                Err(err) => {
                    #[cfg(feature = "telemetry")]
                    crate::telemetry::RECEIPT_PERSIST_FAIL_TOTAL.inc();
                    #[cfg(any(feature = "telemetry", feature = "test-telemetry"))]
                    tracing::error!("receipt insert failed: {err}");
                    #[cfg(all(not(feature = "telemetry"), not(feature = "test-telemetry")))]
                    let _ = err;
                }
            }
        }
        BIDS.lock().unwrap_or_else(|e| e.into_inner()).clear();
        ASKS.lock().unwrap_or_else(|e| e.into_inner()).clear();
        #[cfg(feature = "telemetry")]
        crate::telemetry::MATCH_LOOP_LATENCY_SECONDS.observe(_start.elapsed().as_secs_f64());
        tokio::time::sleep(MATCH_INTERVAL).await;
    }
}
