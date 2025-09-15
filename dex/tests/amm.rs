use dex::amm::Pool;

#[test]
fn swap_preserves_invariant() {
    let mut pool = Pool::new();
    pool.add_liquidity(1000, 1000);
    let k = pool.ct_reserve * pool.it_reserve;
    let _out = pool.swap_ct_for_it(10);
    assert!(pool.ct_reserve * pool.it_reserve <= k);
}

#[test]
fn swap_slippage_bound() {
    let mut pool = Pool::new();
    pool.add_liquidity(500, 500);
    let out = pool.swap_ct_for_it(100);
    // User receives less than proportional due to slippage but non-zero
    assert!(out > 0 && out < 100);
}
