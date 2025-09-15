#![forbid(unsafe_code)]

pub mod cfmm;
pub mod amm;
pub mod liquidity_reward;
pub mod escrow;
pub mod htlc_router;
pub mod router;

#[cfg(doctest)]
#[doc = concat!("```rust\n", include_str!("../examples/escrow.rs"), "\n```")]
mod escrow_example {}
