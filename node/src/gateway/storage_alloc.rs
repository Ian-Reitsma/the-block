use crate::compute_market::scheduler;
use storage::StorageOffer;

/// Allocate shards across storage offers using reputation-weighted
/// proportions. The resulting vector contains `(provider_id, shares)`
/// pairs where `shares` specifies how many Lagrange-coded shards the
/// provider should store.
pub fn allocate(offers: &[StorageOffer], shares: u16) -> Vec<(String, u16)> {
    if offers.is_empty() || shares == 0 {
        return Vec::new();
    }
    let mut weights = Vec::with_capacity(offers.len());
    let mut total = 0f64;
    for o in offers {
        let rep = scheduler::reputation_get(&o.provider_id).max(1) as f64;
        let w = (o.capacity_bytes as f64) * rep;
        total += w;
        weights.push((o.provider_id.clone(), w));
    }
    let mut alloc: Vec<(String, u16)> = weights
        .into_iter()
        .map(|(id, w)| {
            let frac = w / total;
            let n = (frac * shares as f64).round() as u16;
            (id, n.max(1))
        })
        .collect();
    // Adjust rounding to ensure sum equals shares
    let mut assigned: i32 = alloc.iter().map(|(_, n)| *n as i32).sum();
    let mut idx = 0;
    while assigned != shares as i32 && !alloc.is_empty() {
        if assigned > shares as i32 {
            if alloc[idx].1 > 1 {
                alloc[idx].1 -= 1;
                assigned -= 1;
            }
        } else {
            alloc[idx].1 += 1;
            assigned += 1;
        }
        idx = (idx + 1) % alloc.len();
    }
    alloc
}
