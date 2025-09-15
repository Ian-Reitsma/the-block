//! Simple simulation stub exploring multi-pool arbitrage.
use dex::amm::Pool;

pub fn simulate() {
    let mut a = Pool::new();
    let mut b = Pool::new();
    a.add_liquidity(1000, 1000);
    b.add_liquidity(1000, 1000);
    let _ = a.swap_ct_for_it(50);
    let _ = b.swap_it_for_ct(40);
}
