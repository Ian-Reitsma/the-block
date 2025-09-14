#![forbid(unsafe_code)]

use crate::{Explorer, OrderRecord};
use anyhow::Result;

/// Return the current DEX order book.
pub fn list_orders(exp: &Explorer) -> Result<Vec<OrderRecord>> {
    exp.order_book()
}
