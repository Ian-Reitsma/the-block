#![forbid(unsafe_code)]

pub mod audit;
pub mod exchange_hooks;
pub mod order_book;
pub mod storage;
pub mod trust_lines;
pub use dex::escrow;
use std::time::{SystemTime, UNIX_EPOCH};

pub use exchange_hooks::{ExchangeAdapter, OsmosisAdapter, UniswapAdapter};
pub use order_book::{Order, OrderBook, Side};
pub use storage::DexStore;
pub use trust_lines::{TrustLedger, TrustLine};

const MIN_LOCK_SECS: u64 = 60;

pub fn check_liquidity_rules(locked_at: u64) -> Result<(), &'static str> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    if now < locked_at + MIN_LOCK_SECS {
        Err("lock_duration")
    } else {
        Ok(())
    }
}
