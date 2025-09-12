#![forbid(unsafe_code)]

use super::{storage::EscrowState, DexStore};

/// Verify that the escrowed totals match tracked obligations.
pub fn audit(store: &DexStore) -> bool {
    let state: EscrowState = store.load_escrow_state();
    let locked_sum: u64 = state
        .locks
        .values()
        .map(|(_, sell, qty, _)| sell.price * *qty)
        .sum();
    locked_sum == state.escrow.total_locked()
}
