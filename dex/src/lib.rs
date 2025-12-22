#![allow(clippy::manual_div_ceil)]
#![forbid(unsafe_code)]

pub mod amm;
pub mod cfmm;
pub mod escrow;
pub mod htlc_router;
pub mod liquidity_reward;
pub mod router;

#[cfg(doctest)]
#[doc = concat!("```rust\n", include_str!("../examples/escrow.rs"), "\n```")]
mod escrow_example {}
