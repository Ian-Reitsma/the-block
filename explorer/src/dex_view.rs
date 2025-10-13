#![forbid(unsafe_code)]

use crate::{Explorer, OrderRecord};
use diagnostics::anyhow::{self, Result};

/// Return the current DEX order book.
pub fn list_orders(exp: &Explorer) -> Result<Vec<OrderRecord>> {
    exp.order_book().map_err(anyhow::Error::from_error)
}
